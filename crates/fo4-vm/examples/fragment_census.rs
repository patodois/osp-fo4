#![forbid(unsafe_code)]
//! Census of the VMAD fragment blocks of INFO, PACK, SCEN and PERK records
//! and of the scene records they belong to (`spec/scene-record.md`):
//! `fragment_census MANIFEST OUT.txt NAMES.txt`.
//!
//! OUT.txt holds counts, record type tags and slot kinds only (committed as
//! `spec/fragment-census.txt`); NAMES.txt names the records and scripts
//! behind the counts that need a look and is never committed.
//!
//! Part 1: every block read by `fo4_plugin::vmad` (version byte, flag
//! byte, fragments, the slot each fragment attaches to) and whether the
//! function it names exists in the compiled script, with its parameter
//! types (through `fo4_pex`). Part 2: every winning SCEN read by
//! `fo4_plugin::scen` (phases, conditions, actors, actions by kind, the
//! records their four-byte fields name, the owning quest against the
//! group that holds the record). Part 3: the fragments placed against the
//! records (phase index against the phases, PERK entry index against the
//! PRKE entries, INFO and SCEN owning quests).
use fo4_core::{Budget, Manifest};
use fo4_pex::Pex;
use fo4_plugin::{
    actor::Context,
    scen::Scene,
    typed::Store,
    vmad::{Fragment, Vmad, FRAGMENT_BEGIN, FRAGMENT_CHANGE, FRAGMENT_END},
    FormKey,
};
use fo4_vfs::Vfs;
use fo4_vm::{MapLibrary, Script};
use std::{collections::BTreeMap, fmt::Write as _, path::Path};

type E = Box<dyn std::error::Error>;
type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}
fn add(m: &mut Counts, k: impl Into<String>, n: u64) {
    *m.entry(k.into()).or_default() += n;
}
fn sig(s: [u8; 4]) -> String {
    String::from_utf8_lossy(&s).into_owned()
}
fn holds(c: &mut Counts, what: &str, ok: bool) {
    bump(c, format!("{what}\t{}", if ok { "holds" } else { "fails" }));
}
fn bucket(n: usize) -> &'static str {
    match n {
        0 => "0",
        1 => "1",
        2 => "2",
        3..=4 => "3-4",
        5..=9 => "5-9",
        10..=19 => "10-19",
        _ => ">=20",
    }
}
fn slot_name(bit: u8) -> &'static str {
    match bit {
        FRAGMENT_BEGIN => "begin",
        FRAGMENT_END => "end",
        FRAGMENT_CHANGE => "change",
        _ => "other",
    }
}
fn lower(b: &[u8]) -> String {
    fo4_pex::index::lower(b)
}

/// The function a fragment names, looked up in the empty state of the
/// script and its parents.
fn function<'l>(lib: &'l MapLibrary, script: &str, f: &str) -> Option<&'l fo4_vm::Func> {
    let mut at = script.to_string();
    for _ in 0..64 {
        let s = lib.scripts.get(&at)?;
        if let Some(x) = s.functions.get(&(String::new(), f.to_string())) {
            return Some(x);
        }
        if s.parent.is_empty() {
            return None;
        }
        at = s.parent.clone();
    }
    None
}

/// Counts one fragment: the script against the block's, the stored byte,
/// and the function in the compiled script.
fn fragment(
    c: &mut Counts,
    names: &mut String,
    lib: &MapLibrary,
    (t, slot): (&str, &str),
    block: &[u8],
    f: &Fragment<'_>,
    who: &str,
) {
    bump(c, format!("1 fragments by slot\t{t}\t{slot}"));
    bump(
        c,
        format!("1 fragment byte before the texts\t{t}\t{}", f.byte),
    );
    holds(
        c,
        &format!("1 fragment script is the block's script\t{t}"),
        f.script == block,
    );
    let script = lower(f.script);
    let name = lower(f.function);
    match function(lib, &script, &name) {
        None => {
            bump(
                c,
                format!("1 fragment function in the compiled script\t{t}\tnot found"),
            );
            let _ = writeln!(names, "MISSING {t} {who}: {script}.{name}");
        }
        Some(x) => {
            let params: Vec<&str> = x.params.iter().map(|(_, ty)| ty.as_str()).collect();
            let kind = |ty: &str| -> String {
                match ty {
                    "int" | "float" | "bool" | "string" | "var" => ty.into(),
                    t if t.ends_with("[]") => "an array".into(),
                    t => t.into(),
                }
            };
            bump(
                c,
                format!(
                    "1 fragment function in the compiled script\t{t}\tfound: parameters ({}) returns {}, {} instructions",
                    params.iter().map(|p| kind(p)).collect::<Vec<_>>().join(", "),
                    if x.return_type.is_empty() { "none" } else { &x.return_type },
                    if x.code.is_empty() { "no" } else { "with" }
                ),
            );
        }
    }
    // The stored function name's last part against the slot.
    let suffix = name.rsplit('_').next().unwrap_or("");
    bump(
        c,
        format!("1 function name ends with\t{t}\t{slot}\t_{suffix}"),
    );
}

fn run() -> Result<(), E> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 3 {
        return Err("fragment_census MANIFEST OUT.txt NAMES.txt".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let b = Budget::new(m.limits.max_memory);
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let mut c = Counts::new();
    let mut names = String::new();
    let label = |k: &FormKey| -> String {
        let e = store
            .raw(k, &b.child())
            .ok()
            .flatten()
            .and_then(|r| fo4_vm::bind::editor_id(&r))
            .unwrap_or_default();
        format!("{}:{:06x} {e}", k.plugin, k.object_id)
    };
    // Compiled scripts.
    let mut lib = MapLibrary::default();
    {
        let cands = &vfs.candidates;
        for (i, x) in cands.iter().enumerate() {
            let Some(k) = &x.key else { continue };
            if !k.ends_with(b".pex") || cands.get(i + 1).and_then(|n| n.key.as_ref()) == Some(k) {
                continue;
            }
            let bb = Budget::new(m.limits.max_memory);
            let bytes = vfs.read_candidate(x, &bb)?;
            let path = String::from_utf8_lossy(k).into_owned();
            let p = Pex::read(&bytes, &path, &bb)?;
            if let Some(s) = Script::from_pex(&p, 0) {
                lib.add(s);
            }
        }
    }
    // The group of kind 10 holding each winning record (its label).
    let mut group10: BTreeMap<FormKey, FormKey> = BTreeMap::new();
    for (o, cell, kind) in store.placement_groups() {
        if kind == 10 && matches!(&o.signature, b"INFO" | b"SCEN" | b"DIAL" | b"DLBR") {
            group10.insert(store.overrides().form_key(o.key, store.order()), cell);
        }
    }
    let winners: Vec<(FormKey, [u8; 4])> = store
        .winners()
        .filter(|w| !w.deleted && matches!(&w.signature, b"INFO" | b"PACK" | b"SCEN" | b"PERK"))
        .map(|w| {
            (
                store.overrides().form_key(w.key, store.order()),
                w.signature,
            )
        })
        .collect();
    for (k, s) in &winners {
        let t = sig(*s);
        let bb = b.child();
        let Some((r, plugin)) = store.raw_with_plugin(k, &bb)? else {
            continue;
        };
        bump(&mut c, format!("0 winning records\t{t}"));
        let who = label(k);
        // The owning quest from the holding group.
        if *s == *b"INFO" || *s == *b"SCEN" {
            let what = match group10.get(k) {
                None => "held by no group of kind 10".to_string(),
                Some(q) => match store.signature(q) {
                    Some(x) => format!("held by a group of kind 10 labelled with a {}", sig(x)),
                    None => "held by a group of kind 10 whose label names no winning record".into(),
                },
            };
            bump(&mut c, format!("3 owning quest\t{t}\t{what}"));
        }
        // ---- SCEN record --------------------------------------------------
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        let mut scene = None;
        if *s == *b"SCEN" {
            match Scene::read(&r, ctx, &bb) {
                Err(e) => {
                    bump(&mut c, format!("2 SCEN read errors\t{}", e.code));
                    let _ = writeln!(names, "SCEN ERROR {who}: {e}");
                }
                Ok(x) => {
                    scene_counts(&mut c, &store, &x, k, group10.get(k));
                    scene = Some(x);
                }
            }
        }
        // ---- VMAD blocks --------------------------------------------------
        let Some(sub) = r.subrecords.iter().find(|x| x.signature == *b"VMAD") else {
            continue;
        };
        bump(&mut c, format!("0 records with a VMAD\t{t}"));
        let v = match Vmad::read(r.data(sub), r.signature, &r.source.file, sub.offset, &bb) {
            Ok(v) => v,
            Err(e) => {
                bump(&mut c, format!("1 VMAD read errors\t{t}\t{}", e.code));
                let _ = writeln!(names, "VMAD ERROR {who}: {e}");
                continue;
            }
        };
        holds(
            &mut c,
            &format!("1 VMAD read to its last byte\t{t}"),
            v.tail.is_empty(),
        );
        let flagged = v
            .info
            .as_ref()
            .or(v.package.as_ref())
            .or(v.scene.as_ref().map(|x| &x.block));
        if let Some(bl) = flagged {
            bump(&mut c, format!("1 blocks\t{t}"));
            bump(&mut c, format!("1 block version byte\t{t}\t{}", bl.version));
            bump(&mut c, format!("1 block flag byte\t{t}\t{:#04x}", bl.flags));
            bump(
                &mut c,
                format!(
                    "1 fragments per block (begin, end, change)\t{t}\t{}",
                    bl.fragments.len()
                ),
            );
            bump(
                &mut c,
                format!(
                    "1 block script names a compiled script\t{t}\t{}",
                    lib.scripts.contains_key(&lower(bl.script.name))
                ),
            );
            add(
                &mut c,
                format!("1 block script properties\t{t}"),
                bl.script.properties.len() as u64,
            );
            for (bit, f) in &bl.fragments {
                fragment(
                    &mut c,
                    &mut names,
                    &lib,
                    (&t, slot_name(*bit)),
                    bl.script.name,
                    f,
                    &who,
                );
            }
        } else if *s != *b"PERK" {
            bump(&mut c, format!("1 VMAD without a block\t{t}"));
        }
        if let Some(sc) = &v.scene {
            bump(
                &mut c,
                format!(
                    "1 phase fragments per SCEN block\t{}",
                    bucket(sc.phases.len())
                ),
            );
            for p in &sc.phases {
                let name = lower(p.fragment.function);
                let suffix = name.rsplit('_').next().unwrap_or("").to_string();
                bump(
                    &mut c,
                    format!(
                        "1 phase fragment flag byte against the function name\t{}\t_{suffix}",
                        p.flags
                    ),
                );
                let slot = match p.flags {
                    1 => "phase begin (flag 1)",
                    2 => "phase end (flag 2)",
                    _ => "phase, flag byte neither 1 nor 2",
                };
                if p.flags != 1 && p.flags != 2 {
                    let _ = writeln!(
                        names,
                        "PHASE FLAG {} {who}: {name} phase {}",
                        p.flags, p.phase
                    );
                }
                fragment(
                    &mut c,
                    &mut names,
                    &lib,
                    ("SCEN", slot),
                    sc.block.script.name,
                    &p.fragment,
                    &who,
                );
                // The phase index against the record's phases and against
                // the number in the function's name.
                if let Some(x) = &scene {
                    holds(
                        &mut c,
                        "3 phase fragment index is below the number of phases",
                        (p.phase as usize) < x.phases.len(),
                    );
                }
                let nn: Option<u32> = name
                    .split('_')
                    .find_map(|w| (w.len() == 2).then(|| w.parse().ok()).flatten());
                holds(
                    &mut c,
                    "3 phase fragment index + 1 equals the two digits in the function name",
                    nn == Some(p.phase + 1),
                );
            }
            // Two fragments of one phase and flag.
            let mut seen = std::collections::BTreeSet::new();
            let dup = sc.phases.iter().any(|p| !seen.insert((p.phase, p.flags)));
            holds(
                &mut c,
                "3 at most one phase fragment per phase and flag",
                !dup,
            );
        }
        if let Some(pk) = &v.perk {
            bump(&mut c, "1 blocks\tPERK");
            bump(
                &mut c,
                format!("1 block version byte\tPERK\t{}", pk.version),
            );
            bump(
                &mut c,
                format!("1 fragments per block\tPERK\t{}", pk.fragments.len()),
            );
            bump(
                &mut c,
                format!(
                    "1 block script names a compiled script\tPERK\t{}",
                    lib.scripts.contains_key(&lower(pk.script.name))
                ),
            );
            // PRKE entries of the record, in stored order, with their
            // first byte.
            let entries: Vec<u8> = r
                .subrecords
                .iter()
                .filter(|x| x.signature == *b"PRKE")
                .map(|x| r.data(x).first().copied().unwrap_or(0xff))
                .collect();
            for f in &pk.fragments {
                bump(
                    &mut c,
                    format!("1 PERK fragment word after the index\t{}", f.word),
                );
                fragment(
                    &mut c,
                    &mut names,
                    &lib,
                    ("PERK", "perk entry"),
                    pk.script.name,
                    &f.fragment,
                    &who,
                );
                let e = entries.get(f.entry as usize);
                holds(
                    &mut c,
                    "3 PERK fragment index is below the number of PRKE entries",
                    e.is_some(),
                );
                if let Some(e) = e {
                    bump(
                        &mut c,
                        format!("3 PERK fragment's PRKE entry, first byte\t{e}"),
                    );
                }
                let nn: Option<u16> = lower(f.fragment.function)
                    .rsplit('_')
                    .next()
                    .and_then(|w| w.parse().ok());
                holds(
                    &mut c,
                    "3 PERK fragment index equals the digits ending the function name",
                    nn == Some(f.entry),
                );
            }
        }
    }
    let mut s = String::new();
    writeln!(
        s,
        "# fragment_census: counts, record type tags, slot kinds and parameter types only (spec/scene-record.md)"
    )?;
    writeln!(
        s,
        "# 0 records; 1 VMAD fragment blocks and their functions; 2 scene records; 3 fragments placed against the records"
    )?;
    for (k, v) in &c {
        writeln!(s, "{v}\t{k}")?;
    }
    std::fs::write(&a[1], s)?;
    std::fs::write(&a[2], names)?;
    Ok(())
}

/// Counts of one scene record.
fn scene_counts(c: &mut Counts, store: &Store, x: &Scene, k: &FormKey, group: Option<&FormKey>) {
    let _ = k;
    bump(c, "2 SCEN read to the last byte");
    let what = |f: &fo4_plugin::typed::FormRef| -> String {
        match (&f.key, f.raw) {
            (_, 0) => "zero".into(),
            (Some(k), _) => store
                .signature(k)
                .map(sig)
                .unwrap_or_else(|| "a FormID naming no winning record".into()),
            (None, _) => "a word that does not resolve".into(),
        }
    };
    bump(c, format!("2 PNAM names\t{}", what(&x.quest)));
    holds(
        c,
        "3 SCEN PNAM equals the label of the group of kind 10 holding it",
        group.is_some() && x.quest.key.as_ref() == group,
    );
    for bit in 0..32 {
        if x.flags & (1 << bit) != 0 {
            bump(c, format!("2 scene FNAM bit set\t{:#010x}", 1u32 << bit));
        }
    }
    bump(c, format!("2 phases per scene\t{}", bucket(x.phases.len())));
    bump(c, format!("2 actors per scene\t{}", bucket(x.actors.len())));
    bump(
        c,
        format!("2 actions per scene\t{}", bucket(x.actions.len())),
    );
    for p in &x.phases {
        bump(
            c,
            format!(
                "2 phase conditions (start, completion)\t{}, {}",
                bucket(p.start.len()),
                bucket(p.completion.len())
            ),
        );
        for cd in &p.start {
            bump(
                c,
                format!(
                    "2 phase start condition function index\t{}",
                    cd.ctda.function
                ),
            );
        }
        for cd in &p.completion {
            bump(
                c,
                format!(
                    "2 phase completion condition function index\t{}",
                    cd.ctda.function
                ),
            );
        }
    }
    let n = x.phases.len() as u32;
    let mut index = std::collections::BTreeSet::new();
    for a in &x.actions {
        bump(c, format!("2 actions by kind\t{}", a.kind));
        holds(
            c,
            &format!(
                "2 action start phase <= end phase < number of phases\tkind {}",
                a.kind
            ),
            a.start <= a.end && a.end < n,
        );
        holds(
            c,
            "2 action ALID is an actor's ALID or -1",
            a.alias == -1 || x.actors.iter().any(|y| y.alias == a.alias),
        );
        index.insert(a.index);
        if let Some(f) = a.flags {
            for bit in 0..32 {
                if f & (1 << bit) != 0 {
                    bump(
                        c,
                        format!(
                            "2 action FNAM bit set\tkind {}\t{:#010x}",
                            a.kind,
                            1u32 << bit
                        ),
                    );
                }
            }
        }
        if !a.conditions.is_empty() {
            add(
                c,
                format!("2 action conditions\tkind {}", a.kind),
                a.conditions.len() as u64,
            );
        }
        for f in &a.fields {
            let named = match &f.form {
                Some(fr) => what(fr),
                None => format!("{} bytes", f.data.len()),
            };
            bump(
                c,
                format!("2 action fields\tkind {}\t{}\t{named}", a.kind, sig(f.tag)),
            );
        }
    }
    holds(
        c,
        "2 action INAM distinct within a scene",
        index.len() == x.actions.len(),
    );
    if let Some(max) = index.iter().max() {
        holds(
            c,
            "2 scene INAM greater than every action INAM",
            x.inam > *max,
        );
    }
    for f in &x.fields {
        let what = if f.unsettled.is_some() {
            "kept as stored"
        } else {
            "typed"
        };
        add(
            c,
            format!(
                "2 bytes by place and tag\t{}\t{}\t{what}",
                f.place,
                sig(f.tag)
            ),
            f.size as u64,
        );
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
