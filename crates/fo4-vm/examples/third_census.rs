#![forbid(unsafe_code)]
//! What the data holds behind the stops of the third fight's step
//! (`spec/vm.md`, The third fight): `third_census MANIFEST OUT.txt
//! NAMES.txt [--quest EDITOR_ID] [--scene PLUGIN:OBJECTID]...
//! [--function N]`.
//!
//! OUT.txt holds counts and kinds only (`spec/quest-run-third-census.txt`);
//! NAMES.txt names records and scripts and is never versioned. Nothing
//! runs. Sections:
//!
//! 1. aliases created at run time (`ALCO`, `ALCA`, `ALCL`): by alias kind,
//!    the created object's record type, the alias the `ALCA` low half
//!    names (where the object is created) and how that alias is filled,
//!    the `ALCA` high half, the `ALCL` word, the other values the entry
//!    carries, its flag bits, and whether its quest starts with the game
//!    (with `--quest`, the quest's own);
//! 2. aliases filled from another quest's alias (`ALEQ`, `ALEA`): by alias
//!    kind, the quest and alias named and how that alias is filled,
//!    whether either quest starts with the game, the flag bits, the other
//!    values (with `--quest`, the quest's own);
//! 3. each `--scene` (a scene, or any record that names one; numbered in
//!    the order given): every winning record whose subrecords hold a word
//!    that resolves to it, by record type and tag (its own record left
//!    out), and for script property values naming it (the record's
//!    scripts, a quest's fragment script and alias scripts), whether a
//!    script of the chain calls `Start` or `ForceStart` on that property's
//!    variable; a scene's own quest (its first `PNAM`) and flag word;
//! 4. with `--function N`: every condition (CTDA) of that function index
//!    in every winning record, by host record type, run-on word, operator,
//!    comparison value, the first parameter's kind and the last word.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{
    actor::Context,
    condition::{lists, Comparison},
    quest::{AliasKind, AliasValue, Quest},
    typed::Store,
    vmad::{PropertyValue, Vmad},
    FormKey,
};
use fo4_vfs::Vfs;
use fo4_vm::{bind::record_key, Lit, MapLibrary};
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

/// The record type of a key, as text.
fn sig_text(store: &Store, k: Option<&FormKey>) -> String {
    match k.and_then(|k| store.signature(k)) {
        Some(s) => String::from_utf8_lossy(&s).to_string(),
        None => "no winning record".into(),
    }
}

/// How an alias entry is filled, by the values it carries.
fn fill_kind(q: &Quest, id: i64) -> String {
    let Some(al) = q.aliases.iter().find(|x| i64::from(x.id) == id) else {
        return "an alias the quest does not list".into();
    };
    if al.kind == AliasKind::Location {
        return "a location alias".into();
    }
    let mut kinds: Vec<&str> = al
        .values
        .iter()
        .filter_map(|v| match v {
            AliasValue::ForcedReference(_) => Some("a forced reference"),
            AliasValue::UniqueActor(_) => Some("a unique actor"),
            AliasValue::LocationAliasReference { .. } => Some("a location's reference"),
            AliasValue::FromEvent { .. } => Some("a story event's data"),
            AliasValue::CreateObject { .. } => Some("a created object"),
            AliasValue::External { .. } => Some("another quest's alias"),
            AliasValue::LinkedAlias { .. } => Some("a linked alias"),
            _ => None,
        })
        .collect();
    kinds.dedup();
    let collection = matches!(al.kind, AliasKind::Collection { .. });
    let base = if kinds.is_empty() {
        "another fill (conditions or none)".to_string()
    } else {
        kinds.join(" and ")
    };
    if collection {
        format!("a collection alias: {base}")
    } else {
        base
    }
}

fn kind_text(k: AliasKind) -> &'static str {
    match k {
        AliasKind::Reference => "reference alias",
        AliasKind::Location => "location alias",
        AliasKind::Collection { .. } => "collection alias",
    }
}

/// The flag bits of a word, one entry per bit set (or `no bit`).
fn bits(w: u32) -> Vec<String> {
    let v: Vec<String> = (0..32)
        .filter(|b| w & (1 << b) != 0)
        .map(|b| format!("bit 0x{:x}", 1u32 << b))
        .collect();
    if v.is_empty() {
        vec!["no bit".into()]
    } else {
        v
    }
}

/// The tags of an alias entry's values other than the fill itself.
fn other_tags(values: &[AliasValue], fill: [u8; 4]) -> BTreeSet<String> {
    values
        .iter()
        .map(AliasValue::tag)
        .filter(|t| *t != fill)
        .map(|t| String::from_utf8_lossy(&t).to_string())
        .collect()
}

fn start_game(q: &Quest) -> bool {
    q.general
        .is_some_and(|g| g.flags & fo4_vm::quest::START_GAME_ENABLED != 0)
}

/// Whether a script of the chain calls `Start` or `ForceStart` on the
/// variable behind a property (opcode 23 with that receiver).
fn starts_property(lib: &MapLibrary, script: &str, property: &str) -> bool {
    let mut t = script.to_string();
    let var = |s: &fo4_vm::Script| -> Option<String> {
        s.properties
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(property))
            .and_then(|p| p.variable.clone())
    };
    let mut vars: BTreeSet<String> = BTreeSet::new();
    let mut chain = Vec::new();
    for _ in 0..fo4_vm::MAX_PARENTS {
        let Some(s) = lib.scripts.get(&t) else { break };
        if let Some(v) = var(s) {
            vars.insert(v);
        }
        chain.push(s.clone());
        if s.parent.is_empty() {
            break;
        }
        t = s.parent.clone();
    }
    vars.insert(property.to_ascii_lowercase());
    chain.iter().any(|s| {
        s.functions.values().any(|f| {
            f.code.iter().any(|i| {
                i.op == 23
                    && matches!(i.args.first(), Some(Lit::Ident(n)) if n == "start" || n == "forcestart")
                    && matches!(i.args.get(1), Some(Lit::Ident(r)) if vars.contains(r))
            })
        })
    })
}

fn run() -> Result<(), E> {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let usage = "third_census MANIFEST OUT.txt NAMES.txt [--quest EDITOR_ID] [--scene PLUGIN:OBJECTID]... [--function N]";
    let mut quest_id = None;
    let mut scenes: Vec<FormKey> = Vec::new();
    let mut function: Option<u16> = None;
    while let Some(i) = a
        .iter()
        .position(|x| x == "--quest" || x == "--scene" || x == "--function")
    {
        let v = a.get(i + 1).cloned().ok_or(usage)?;
        match a[i].as_str() {
            "--quest" => quest_id = Some(v),
            "--scene" => scenes.push(fo4_vm::player::parse_key(&v).ok_or(usage)?),
            _ => function = Some(v.parse()?),
        }
        a.drain(i..i + 2);
    }
    if a.len() != 3 {
        return Err(usage.into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let lib = fo4_vm::setters::read_all_scripts(&vfs, m.limits.max_memory)?;
    let max = m.limits.max_memory;
    let mut c = Counts::new();
    let mut names = String::new();

    // Every winning quest, read once.
    let mut read: BTreeMap<FormKey, Quest> = BTreeMap::new();
    for o in store.winners() {
        if o.deleted || o.signature != *b"QUST" {
            continue;
        }
        let k = store.overrides().form_key(o.key, store.order());
        let b = Budget::new(max);
        let Some((r, plugin)) = store.raw_with_plugin(&k, &b)? else {
            continue;
        };
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        if let Ok(q) = Quest::read(&r, ctx, &b) {
            read.insert(k, q);
        }
    }
    let target = quest_id.as_ref().and_then(|e| {
        read.iter()
            .find(|(_, q)| {
                q.editor_id
                    .as_deref()
                    .is_some_and(|x| x.eq_ignore_ascii_case(e.as_bytes()))
            })
            .map(|(k, _)| k.clone())
    });

    // 1 and 2: created and borrowed aliases.
    for (k, q) in &read {
        let own = target.as_ref() == Some(k);
        for al in &q.aliases {
            for v in &al.values {
                match v {
                    AliasValue::CreateObject {
                        object,
                        alias,
                        word,
                        level,
                    } => {
                        let p = "1 aliases created at run time";
                        bump(&mut c, format!("{p}\tby kind\t{}", kind_text(al.kind)));
                        bump(
                            &mut c,
                            format!(
                                "{p}\tby the created object's record type\t{}",
                                sig_text(&store, object.key.as_ref())
                            ),
                        );
                        let place = fill_kind(q, i64::from(*alias));
                        bump(
                            &mut c,
                            format!("{p}\tby the alias ALCA names (where it is created)\t{place}"),
                        );
                        if place == "a forced reference" {
                            let at = q
                                .aliases
                                .iter()
                                .find(|x| x.id == u32::from(*alias))
                                .and_then(|x| x.forced_reference())
                                .and_then(|f| f.key.clone());
                            let base = at
                                .as_ref()
                                .and_then(|r| {
                                    fo4_vm::bind::base_of(&store, r, &Budget::new(max))
                                        .ok()
                                        .flatten()
                                })
                                .map(|b| sig_text(&store, Some(&b)))
                                .unwrap_or_else(|| "no base".into());
                            bump(
                                &mut c,
                                format!(
                                    "{p}\tthe forced reference they are created at: its record type and its base's\t{} {base}",
                                    sig_text(&store, at.as_ref())
                                ),
                            );
                        }
                        bump(&mut c, format!("{p}\tALCA high half\t0x{word:04x}"));
                        bump(&mut c, format!("{p}\tALCL word\t{level}"));
                        for t in other_tags(&al.values, *b"ALCO") {
                            bump(&mut c, format!("{p}\tother values they carry\t{t}"));
                        }
                        for x in bits(al.flags) {
                            bump(&mut c, format!("{p}\tflag bits\t{x}"));
                        }
                        bump(
                            &mut c,
                            format!("{p}\ttheir quest starts with the game\t{}", start_game(q)),
                        );
                        if own {
                            bump(
                                &mut c,
                                format!(
                                    "1 the quest's own created aliases\t{} at {place}, level word {level}",
                                    sig_text(&store, object.key.as_ref())
                                ),
                            );
                        }
                    }
                    AliasValue::External { quest, alias } => {
                        let p = "2 aliases filled from another quest's alias";
                        bump(&mut c, format!("{p}\tby kind\t{}", kind_text(al.kind)));
                        let other = quest.key.as_ref().and_then(|x| read.get(x));
                        let named = match other {
                            Some(o) => fill_kind(o, i64::from(*alias)),
                            None => "a quest the load order does not hold".into(),
                        };
                        bump(
                            &mut c,
                            format!("{p}\tby how the alias they name is filled\t{named}"),
                        );
                        if quest.key.as_ref() == Some(k) {
                            bump(&mut c, format!("{p}\tnaming their own quest"));
                        }
                        bump(
                            &mut c,
                            format!(
                                "{p}\ttheir quest starts with the game, the quest named does\t{}, {}",
                                start_game(q),
                                other.map(start_game).map(|b| b.to_string()).unwrap_or("-".into())
                            ),
                        );
                        for t in other_tags(&al.values, *b"ALEQ") {
                            bump(&mut c, format!("{p}\tother values they carry\t{t}"));
                        }
                        for x in bits(al.flags) {
                            bump(&mut c, format!("{p}\tflag bits\t{x}"));
                        }
                        if own {
                            bump(
                                &mut c,
                                format!("2 the quest's own borrowed aliases\t{named}"),
                            );
                            let _ = writeln!(
                                names,
                                "## the quest's alias {} names alias {alias} of {}",
                                al.id,
                                quest.key.as_ref().map(record_key).unwrap_or_default()
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // 3 scenes: every winning record holding a word that resolves to one.
    if !scenes.is_empty() {
        let wanted: BTreeSet<u32> = scenes.iter().map(|k| k.object_id & 0x00ff_ffff).collect();
        let mut hits: Vec<BTreeMap<String, u64>> = vec![BTreeMap::new(); scenes.len()];
        let mut scanned = 0u64;
        for o in store.winners() {
            if o.deleted {
                continue;
            }
            let k = store.overrides().form_key(o.key, store.order());
            let b = Budget::new(max);
            let Ok(Some((r, plugin))) = store.raw_with_plugin(&k, &b) else {
                continue;
            };
            scanned += 1;
            for s in &r.subrecords {
                let d = r.data(s);
                if d.len() < 4 {
                    continue;
                }
                for off in 0..=d.len() - 4 {
                    let w = u32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]);
                    if !wanted.contains(&(w & 0x00ff_ffff)) {
                        continue;
                    }
                    let Ok(Some(f)) = store.order().remap(plugin, w, &r.source.file, s.offset)
                    else {
                        continue;
                    };
                    let Some(n) = scenes.iter().position(|x| *x == f) else {
                        continue;
                    };
                    if k == scenes[n] {
                        continue;
                    }
                    let tag = String::from_utf8_lossy(&s.signature).to_string();
                    let what = format!("{}\t{tag}", String::from_utf8_lossy(&r.signature));
                    *hits[n].entry(what).or_default() += 1;
                    let _ = writeln!(
                        names,
                        "## record {} named by {} {} in {tag} at byte {off}",
                        n + 1,
                        String::from_utf8_lossy(&r.signature),
                        record_key(&k)
                    );
                    if s.signature == *b"VMAD" {
                        if let Ok(v) = Vmad::read(d, r.signature, &r.source.file, s.offset, &b) {
                            // The record's scripts, the quest block's
                            // fragment script and its aliases' scripts.
                            let mut all: Vec<(&str, &fo4_plugin::vmad::Script<'_>)> = v
                                .scripts
                                .iter()
                                .map(|x| ("the record's own scripts", x))
                                .collect();
                            if let Some(qb) = &v.quest {
                                all.push(("the quest's fragment script", &qb.script));
                                for al in &qb.aliases {
                                    all.extend(
                                        al.scripts.iter().map(|x| ("a quest alias's scripts", x)),
                                    );
                                }
                            }
                            for (where_, sc) in all {
                                for p in &sc.properties {
                                    let PropertyValue::Object(ov) = &p.value else {
                                        continue;
                                    };
                                    let to = store
                                        .order()
                                        .remap(plugin, ov.form_id, &r.source.file, s.offset)
                                        .ok()
                                        .flatten();
                                    if to.as_ref() != Some(&scenes[n]) {
                                        continue;
                                    }
                                    let script = fo4_pex::index::lower(sc.name);
                                    let prop = String::from_utf8_lossy(p.name).to_string();
                                    let calls = starts_property(&lib, &script, &prop);
                                    bump(
                                        &mut c,
                                        format!(
                                            "3 record {}\ta script property naming it\tin {where_}; whose script calls Start or ForceStart on it: {calls}",
                                            n + 1
                                        ),
                                    );
                                    let _ = writeln!(
                                        names,
                                        "    property {prop} of {script}: Start or ForceStart called on it: {calls}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        let _ = writeln!(names, "## {scanned} winning records scanned");
        for (n, h) in hits.iter().enumerate() {
            if h.is_empty() {
                bump(
                    &mut c,
                    format!("3 record {}\tno other record holds a word naming it", n + 1),
                );
            }
            for (what, v) in h {
                *c.entry(format!(
                    "3 record {}\twords naming it by record type and tag\t{what}",
                    n + 1
                ))
                .or_default() += v;
            }
            let b = Budget::new(max);
            if let Some((r, plugin)) = store.raw_with_plugin(&scenes[n], &b)? {
                let mut first_pnam = true;
                for s in &r.subrecords {
                    let d = r.data(s);
                    match (&s.signature, d.len()) {
                        (b"PNAM", 4) if first_pnam => {
                            first_pnam = false;
                            let w = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
                            let q = store
                                .order()
                                .remap(plugin, w, &r.source.file, s.offset)
                                .ok()
                                .flatten();
                            let starts = q
                                .as_ref()
                                .and_then(|q| read.get(q))
                                .map(start_game)
                                .map(|b| b.to_string())
                                .unwrap_or("-".into());
                            bump(
                                &mut c,
                                format!(
                                    "3 record {}\tits own quest (PNAM): record type, starts with the game\t{}, {starts}",
                                    n + 1,
                                    sig_text(&store, q.as_ref())
                                ),
                            );
                        }
                        (b"FNAM", 4) => {
                            let w = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
                            for x in bits(w) {
                                bump(
                                    &mut c,
                                    format!("3 record {}\tits flag word (FNAM)\t{x}", n + 1),
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // 4 one condition function across every winning record.
    if let Some(func) = function {
        let p = format!("4 conditions of function {func}");
        for o in store.winners() {
            if o.deleted {
                continue;
            }
            let k = store.overrides().form_key(o.key, store.order());
            let b = Budget::new(max);
            let Ok(Some((r, plugin))) = store.raw_with_plugin(&k, &b) else {
                continue;
            };
            if !r.subrecords.iter().any(|s| s.signature == *b"CTDA") {
                continue;
            }
            let Ok(ls) = lists(&r, &b) else { continue };
            let host = String::from_utf8_lossy(&r.signature).to_string();
            for l in &ls {
                for cd in &l.conditions {
                    let x = &cd.ctda;
                    if x.function != func {
                        continue;
                    }
                    bump(&mut c, format!("{p}\tby host record type\t{host}"));
                    bump(&mut c, format!("{p}\tby run-on word\t{}", x.run_on));
                    bump(&mut c, format!("{p}\tby operator\t{}", x.operator()));
                    bump(
                        &mut c,
                        format!(
                            "{p}\tby comparison\t{}",
                            match x.comparison() {
                                Comparison::Float(f) => format!("the number {f}"),
                                Comparison::Global(_) => "a global".into(),
                            }
                        ),
                    );
                    let ctx = Context {
                        order: store.order(),
                        plugin,
                    };
                    let p1 = match fo4_plugin::condition::form(ctx, x.param1, &r.source.file, 0) {
                        _ if x.param1 == 0 => "zero".to_string(),
                        Ok(Some(f)) if store.signature(&f).is_some() => sig_text(&store, Some(&f)),
                        _ if x.param1 < 0x800 => format!("the number {}", x.param1),
                        _ => "a word that names no winning record".into(),
                    };
                    bump(&mut c, format!("{p}\tby the first parameter's kind\t{p1}"));
                    bump(
                        &mut c,
                        format!("{p}\tby host type and first parameter\t{host}\t{p1}"),
                    );
                    bump(
                        &mut c,
                        format!("{p}\tby the second parameter\t{}", x.param2),
                    );
                    bump(&mut c, format!("{p}\tby the last word\t{}", x.param3));
                    for f in bits(u32::from(x.flags())) {
                        bump(&mut c, format!("{p}\tby flag bits\t{f}"));
                    }
                    let _ = writeln!(
                        names,
                        "## function {func} in {host} {}: run-on {} param1 0x{:08x} ({p1})",
                        record_key(&k),
                        x.run_on,
                        x.param1
                    );
                }
            }
        }
    }

    let mut out = String::from(
        "# third_census: kinds and counts only (spec/vm.md, The third fight). Sections: 1 aliases created at run time; 2 aliases filled from another\n# quest's alias; 3 the records named on the command line (numbered): the records holding a word that names them; 4 one condition function's conditions.\n",
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
