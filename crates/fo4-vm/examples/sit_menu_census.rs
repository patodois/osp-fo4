#![forbid(unsafe_code)]
//! Furniture use and menus in the compiled scripts (`spec/vm.md`,
//! Furniture use and Menus): `sit_menu_census MANIFEST OUT.txt NAMES.txt
//! [--family EDITOR_ID_PREFIX]`.
//!
//! OUT.txt holds counts and kinds only (`spec/sit-menu-census.txt`);
//! NAMES.txt names scripts and records and is never versioned. Nothing
//! runs. The event and native names are read from the scripts (the
//! functions without code that scripts declaring natives declare), never
//! from a list of the game's:
//!
//! 1. the handlers with code of the furniture events (event names holding
//!    `sit`, `getup` or `furniture`, `position` left out): by event, by the
//!    engine type the script extends, by state, by parameter types;
//! 2. what those handlers call, by engine call name ([`CALLS`]; any other
//!    call counted as one kind);
//! 3. the attachments of a script declaring such a handler: on placed
//!    references and base records by record type, on quest aliases by the
//!    alias's fill;
//! 4. call sites over every script of the natives whose name holds
//!    `furniture` or `sit` (`position` left out), and of `Activate` by
//!    the literal of its default-processing argument;
//! 5. menus: natives whose name holds `menu` or `chargen`, by call sites
//!    over every script; the handlers with code of events holding `menu`;
//!    the menu registrations by whether their name argument is a literal,
//!    and how many distinct literal names there are (the names only in
//!    NAMES.txt);
//! 6. with `--family`: the same calls and handlers in the scripts the
//!    quests of that editor ID prefix attach (their own, their aliases',
//!    their stage fragment script) and in those scripts' ancestors.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{
    actor::Context,
    quest::{AliasValue, Quest},
    typed::Store,
    vmad::Vmad,
    FormKey,
};
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

/// Engine call names counted one by one inside the furniture handlers.
const CALLS: [&str; 22] = [
    "setstage",
    "getstagedone",
    "getowningquest",
    "getref",
    "getreference",
    "getactorref",
    "gotostate",
    "getsitstate",
    "isfurnitureinuse",
    "getfurnitureref",
    "activate",
    "blockactivation",
    "playanimation",
    "playidle",
    "evaluatepackage",
    "registerforanimationevent",
    "registerforremoteevent",
    "starttimer",
    "start",
    "stop",
    "getplayer",
    "moveto",
];

fn is_furniture_name(n: &str) -> bool {
    !n.contains("position") && (n.contains("sit") || n.contains("getup") || n.contains("furniture"))
}

fn is_menu_name(n: &str) -> bool {
    n.contains("menu") || n.contains("chargen")
}

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

/// The furniture events with code a script or its ancestors declare.
fn declared(lib: &MapLibrary, events: &BTreeSet<String>, script: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for t in chain(lib, script) {
        for ((_, n), f) in &lib.scripts[&t].functions {
            if events.contains(n) && !f.native() && !f.code.is_empty() {
                out.insert(n.clone());
            }
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
        return Err(
            "sit_menu_census MANIFEST OUT.txt NAMES.txt [--family EDITOR_ID_PREFIX]".into(),
        );
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let lib = fo4_vm::setters::read_all_scripts(&vfs, m.limits.max_memory)?;
    let max = m.limits.max_memory;
    let mut c = Counts::new();
    let mut names = String::new();
    // Event names and native names, read from the scripts that declare
    // natives (`setter_event_entry_v1`).
    let mut events: BTreeSet<String> = BTreeSet::new();
    let mut natives: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for s in lib.scripts.values() {
        let has_native = s.functions.values().any(|f| f.native());
        if !has_native {
            continue;
        }
        for ((_, n), f) in &s.functions {
            if f.native() {
                natives.entry(n.clone()).or_default().insert(s.name.clone());
            } else if f.code.is_empty() && n.starts_with("on") {
                events.insert(n.clone());
            }
        }
    }
    let sit_events: BTreeSet<String> = events
        .iter()
        .filter(|n| is_furniture_name(n))
        .cloned()
        .collect();
    let menu_events: BTreeSet<String> = events
        .iter()
        .filter(|n| n.contains("menu"))
        .cloned()
        .collect();
    let sit_natives: BTreeSet<String> = natives
        .keys()
        .filter(|n| is_furniture_name(n))
        .cloned()
        .collect();
    let menu_natives: BTreeSet<String> = natives
        .keys()
        .filter(|n| is_menu_name(n))
        .cloned()
        .collect();
    for (what, set) in [
        ("furniture events", &sit_events),
        ("menu events", &menu_events),
        ("furniture natives", &sit_natives),
        ("menu natives", &menu_natives),
    ] {
        *c.entry(format!("0 names read from the scripts\t{what}"))
            .or_default() += set.len() as u64;
        let _ = writeln!(names, "## {what}: {set:?}");
    }
    *c.entry("0 scripts read".into()).or_default() += lib.scripts.len() as u64;
    // 1, 2 and the menu handlers of 5.
    let mut menu_literals: BTreeSet<String> = BTreeSet::new();
    let mut scripts_by_event: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for s in lib.scripts.values() {
        let ext = extends(&lib, &s.name);
        for ((state, n), f) in &s.functions {
            if f.native() || f.code.is_empty() {
                continue;
            }
            let menu = menu_events.contains(n);
            if !sit_events.contains(n) && !menu {
                continue;
            }
            let sec = if menu { "5 menu" } else { "1 furniture" };
            scripts_by_event
                .entry(n.clone())
                .or_default()
                .insert(s.name.clone());
            bump(&mut c, format!("{sec} handlers with code\t{n}"));
            bump(
                &mut c,
                format!("{sec} handlers with code by type extended\t{n}\t{ext}"),
            );
            let place = if state.is_empty() {
                "the empty state"
            } else if *state == s.auto_state {
                "the script's auto state"
            } else {
                "another state"
            };
            bump(
                &mut c,
                format!("{sec} handlers with code by state\t{n}\t{place}"),
            );
            let params: Vec<&str> = f.params.iter().map(|(_, t)| t.as_str()).collect();
            bump(
                &mut c,
                format!(
                    "{sec} handlers with code by parameter types\t{n}\t({})",
                    params.join(", ")
                ),
            );
            let _ = writeln!(
                names,
                "## handler {n} in {} state {state:?} ({ext})",
                s.name
            );
            if menu {
                continue;
            }
            let mut calls: BTreeSet<&str> = BTreeSet::new();
            for i in &f.code {
                let Some(name) = call_name(i) else { continue };
                let kind = if CALLS.contains(&name) {
                    name
                } else {
                    "another call"
                };
                bump(
                    &mut c,
                    format!("2 calls in the furniture handlers by name\t{n}\t{kind}"),
                );
                calls.insert(kind);
            }
            for k in calls {
                bump(
                    &mut c,
                    format!("2 furniture handlers making at least one call of the name\t{n}\t{k}"),
                );
            }
        }
    }
    for (n, s) in &scripts_by_event {
        let sec = if menu_events.contains(n) {
            "5 menu"
        } else {
            "1 furniture"
        };
        *c.entry(format!("{sec} scripts declaring a handler with code\t{n}"))
            .or_default() += s.len() as u64;
    }
    // 4 and 5: call sites over every script.
    let calls_in = |s: &fo4_vm::Script,
                    c: &mut Counts,
                    sec: &str,
                    lits: &mut BTreeSet<String>,
                    names: &mut String| {
        for f in bodies(s) {
            for i in &f.code {
                let Some(name) = call_name(i) else { continue };
                if sit_natives.contains(name) {
                    bump(c, format!("{sec} furniture native call sites\t{name}"));
                }
                if menu_natives.contains(name) {
                    bump(c, format!("{sec} menu native call sites\t{name}"));
                    if name.contains("register") {
                        let first = i.extra.first();
                        let what = match first {
                            Some(Lit::Str(t)) => {
                                lits.insert(t.to_ascii_lowercase());
                                let _ = writeln!(names, "## {sec}: {} calls {name}({t:?})", s.name);
                                "a literal name"
                            }
                            Some(Lit::Ident(_)) => "a variable or property",
                            _ => "another argument",
                        };
                        bump(
                            c,
                            format!(
                                "{sec} menu registrations by the name argument\t{name}\t{what}"
                            ),
                        );
                    }
                }
                if name == "activate" && i.op == 23 {
                    let what = match i.extra.get(1) {
                        Some(Lit::Bool(true)) => "default processing only: literal true",
                        Some(Lit::Bool(false)) => "default processing only: literal false",
                        None => "default processing only: not given",
                        Some(_) => "default processing only: a variable",
                    };
                    bump(c, format!("{sec} Activate call sites\t{what}"));
                }
            }
        }
    };
    let mut lits_family = BTreeSet::new();
    for s in lib.scripts.values() {
        calls_in(
            s,
            &mut c,
            "4/5 over every script:",
            &mut menu_literals,
            &mut names,
        );
    }
    *c.entry("5 distinct literal names in menu registrations over every script".into())
        .or_default() += menu_literals.len() as u64;
    // 3 and 6: the records attaching them.
    let mut family_scripts: BTreeSet<String> = BTreeSet::new();
    let winners: Vec<FormKey> = store
        .winners()
        .filter(|w| !w.deleted)
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .collect();
    let mut cache: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for k in winners {
        let b = Budget::new(max);
        let Some((r, plugin)) = store.raw_with_plugin(&k, &b)? else {
            continue;
        };
        let Some(s) = r.subrecords.iter().find(|s| s.signature == *b"VMAD") else {
            continue;
        };
        let Ok(v) = Vmad::read(r.data(s), r.signature, &r.source.file, s.offset, &b) else {
            bump(&mut c, "3 VMAD not read");
            continue;
        };
        let sig = String::from_utf8_lossy(&r.signature).into_owned();
        let in_family = r.signature == *b"QUST"
            && family.as_ref().is_some_and(|p| {
                bind::editor_id(&r).is_some_and(|e| e.to_ascii_lowercase().starts_with(p))
            });
        for x in &v.scripts {
            let script = fo4_pex::index::lower(x.name);
            if in_family {
                family_scripts.insert(script.clone());
            }
            let ev = cache
                .entry(script.clone())
                .or_insert_with(|| declared(&lib, &sit_events, &script))
                .clone();
            for e in ev {
                bump(
                    &mut c,
                    format!(
                        "3 attachments of a script with the handler, by record type\t{e}\t{sig}"
                    ),
                );
                let _ = writeln!(names, "## {sig} {} attaches {script} ({e})", record_key(&k));
            }
        }
        if let Some(q) = &v.quest {
            if in_family && !q.script.name.is_empty() {
                family_scripts.insert(fo4_pex::index::lower(q.script.name));
            }
            let quest = if r.signature == *b"QUST" {
                Quest::read(
                    &r,
                    Context {
                        order: store.order(),
                        plugin,
                    },
                    &b,
                )
                .ok()
            } else {
                None
            };
            for al in &q.aliases {
                let id = al.object.alias;
                let fill = quest
                    .as_ref()
                    .and_then(|qq| qq.aliases.iter().find(|x| i64::from(x.id) == i64::from(id)))
                    .map(|x| {
                        let mut kinds: Vec<&str> = x
                            .values
                            .iter()
                            .filter_map(|v| match v {
                                AliasValue::ForcedReference(_) => Some("a forced reference"),
                                AliasValue::UniqueActor(_) => Some("a unique actor"),
                                AliasValue::LocationAliasReference { .. } => {
                                    Some("a location's reference")
                                }
                                AliasValue::FromEvent { .. } => Some("a story event's data"),
                                AliasValue::CreateObject { .. } => Some("a created object"),
                                _ => None,
                            })
                            .collect();
                        kinds.dedup();
                        if kinds.is_empty() {
                            "another fill (conditions, a list or none)".to_string()
                        } else {
                            kinds.join(" and ")
                        }
                    })
                    .unwrap_or_else(|| "an alias the quest record does not list".into());
                for x in &al.scripts {
                    let script = fo4_pex::index::lower(x.name);
                    if in_family {
                        family_scripts.insert(script.clone());
                    }
                    let ev = cache
                        .entry(script.clone())
                        .or_insert_with(|| declared(&lib, &sit_events, &script))
                        .clone();
                    for e in ev {
                        bump(
                            &mut c,
                            format!("3 attachments on quest aliases of a script with the handler, by the alias's fill\t{e}\t{fill}"),
                        );
                        let _ = writeln!(
                            names,
                            "## alias {id} of {} attaches {script} ({e}; {fill})",
                            record_key(&k)
                        );
                    }
                }
            }
        }
    }
    // 6: the family's scripts and their ancestors.
    if family.is_some() {
        let mut all: BTreeSet<String> = BTreeSet::new();
        for s in &family_scripts {
            all.extend(chain(&lib, s));
        }
        *c.entry("6 the family: scripts and ancestors read".into())
            .or_default() += all.len() as u64;
        for t in &all {
            let s = &lib.scripts[t];
            calls_in(s, &mut c, "6 the family:", &mut lits_family, &mut names);
            for ((_, n), f) in &s.functions {
                if !f.native()
                    && !f.code.is_empty()
                    && (sit_events.contains(n) || menu_events.contains(n))
                {
                    bump(&mut c, format!("6 the family: handlers with code\t{n}"));
                }
            }
        }
        *c.entry("6 the family: distinct literal names in menu registrations".into())
            .or_default() += lits_family.len() as u64;
    }
    let mut out = String::from(
        "# sit_menu_census: kinds and counts only (spec/vm.md, Furniture use and Menus). Sections: 0 names read from the scripts; 1 furniture event handlers; 2 what they call; 3 the records attaching them;\n# 4 furniture natives and Activate over every script; 5 menu natives, handlers and registrations; 6 the same in the scripts the opening quest's family attaches.\n",
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
