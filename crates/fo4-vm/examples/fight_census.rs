#![forbid(unsafe_code)]
//! The fight's events and natives in the compiled scripts (`spec/vm.md`,
//! The first fight): `fight_census MANIFEST OUT.txt NAMES.txt
//! [--quest EDITOR_ID]`.
//!
//! OUT.txt holds counts and kinds only (`spec/fight-census.txt`);
//! NAMES.txt names scripts and records and is never versioned. Nothing
//! runs. The event and native names are read from the scripts (the
//! functions without code that scripts declaring natives declare), never
//! from a list of the game's:
//!
//! 1. the handlers with code of the fight events (event names holding
//!    `hit`, `combat`, `death`, `dying` or `kill`): by event, by the engine
//!    type the script extends, by state, by parameter types;
//! 2. what those handlers call, by engine call name ([`CALLS`]; any other
//!    call counted as one kind);
//! 3. the attachments of a script declaring such a handler: on placed
//!    references and base records by record type, on quest aliases by the
//!    alias's fill;
//! 4. call sites over every script of the fight natives (names holding
//!    `dead`, `kill`, `combat`, `hitevent`, `resurrect`, `essential`,
//!    `bleedout` or `health`, `skill` left out), and the event
//!    registrations by the function they stand in (an init handler, the
//!    event's own handler, another);
//! 5. with `--quest`: the quest's aliases whose scripts declare a fight
//!    handler, by event and fill, and the calls those handlers reach (the
//!    callee resolved by the receiver's declared type, `self` and static
//!    calls; at most eight calls deep), natives by name and functions
//!    with code by kind.
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
    rc::Rc,
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

/// Engine call names counted one by one inside the fight handlers.
const CALLS: [&str; 22] = [
    "setstage",
    "getstagedone",
    "getstage",
    "getowningquest",
    "getref",
    "getreference",
    "getactorref",
    "getplayer",
    "gotostate",
    "isdead",
    "kill",
    "isincombat",
    "getcombatstate",
    "getcombattarget",
    "startcombat",
    "stopcombat",
    "registerforhitevent",
    "getdeadcount",
    "getvalue",
    "getvaluepercentage",
    "increment",
    "trytosetstage",
];

/// Most calls followed from one handler.
const MAX_DEPTH: usize = 8;

fn is_fight_event(n: &str) -> bool {
    n.contains("hit")
        || n.contains("combat")
        || n.contains("death")
        || n.contains("dying")
        || n.contains("kill")
}

fn is_fight_native(n: &str) -> bool {
    !n.contains("skill")
        && [
            "dead",
            "kill",
            "combat",
            "hitevent",
            "resurrect",
            "essential",
            "bleedout",
            "health",
        ]
        .iter()
        .any(|x| n.contains(x))
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

/// The fight events with code a script or its ancestors declare.
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
fn bodies(s: &fo4_vm::Script) -> Vec<Rc<fo4_vm::Func>> {
    let mut fs: Vec<_> = s.functions.values().cloned().collect();
    for p in &s.properties {
        fs.extend(p.read.iter().cloned());
        fs.extend(p.write.iter().cloned());
    }
    fs
}

/// The functions of that name a script's chain declares (every state).
fn find(lib: &MapLibrary, script: &str, name: &str) -> Vec<Rc<fo4_vm::Func>> {
    for t in chain(lib, script) {
        let fs: Vec<_> = lib.scripts[&t]
            .functions
            .iter()
            .filter(|((_, n), _)| n == name)
            .map(|(_, f)| f.clone())
            .collect();
        if !fs.is_empty() {
            return fs;
        }
    }
    Vec::new()
}

/// The declared type of an identifier inside a function of a script:
/// a local, a parameter, a script variable, or `self`.
fn type_of(lib: &MapLibrary, script: &str, f: &fo4_vm::Func, id: &str) -> Option<String> {
    if id == "self" {
        return Some(script.to_string());
    }
    if let Some((_, t)) = f.locals.iter().chain(&f.params).find(|(n, _)| n == id) {
        return Some(t.clone());
    }
    for t in chain(lib, script) {
        if let Some(v) = lib.scripts[&t].variables.iter().find(|v| v.name == id) {
            return Some(v.type_name.clone());
        }
    }
    None
}

/// The calls reached from a handler: natives by (script, name), functions
/// with code by name, and calls whose callee is not resolved.
#[derive(Default)]
struct Reach {
    natives: BTreeSet<(String, String)>,
    code: BTreeSet<String>,
    open: BTreeSet<String>,
}

fn reach(
    lib: &MapLibrary,
    script: &str,
    f: &Rc<fo4_vm::Func>,
    depth: usize,
    seen: &mut BTreeSet<String>,
    out: &mut Reach,
) {
    if depth > MAX_DEPTH {
        return;
    }
    for i in &f.code {
        let Some(name) = call_name(i) else { continue };
        let target = match (i.op, i.args.first(), i.args.get(1)) {
            (25, Some(Lit::Ident(s)), _) => Some(s.clone()),
            (23, _, Some(Lit::Ident(recv))) => type_of(lib, script, f, recv),
            (24, _, _) => lib.scripts.get(script).map(|s| s.parent.clone()),
            _ => None,
        };
        let Some(t) = target.filter(|t| lib.scripts.contains_key(t)) else {
            out.open.insert(name.to_string());
            continue;
        };
        let fs = find(lib, &t, name);
        if fs.is_empty() {
            out.open.insert(name.to_string());
            continue;
        }
        for g in fs {
            if g.native() {
                out.natives.insert((g.script.clone(), name.to_string()));
                continue;
            }
            out.code.insert(format!("{}.{}", g.script, name));
            let key = format!("{}.{}.{}", g.script, g.state, name);
            if seen.insert(key) {
                reach(lib, &g.script, &g, depth + 1, seen, out);
            }
        }
    }
}

fn fill_kind(quest: Option<&Quest>, id: i64) -> String {
    quest
        .and_then(|qq| qq.aliases.iter().find(|x| i64::from(x.id) == id))
        .map(|x| {
            let mut kinds: Vec<&str> = x
                .values
                .iter()
                .filter_map(|v| match v {
                    AliasValue::ForcedReference(_) => Some("a forced reference"),
                    AliasValue::UniqueActor(_) => Some("a unique actor"),
                    AliasValue::LocationAliasReference {
                        ref_type: Some(_), ..
                    } => Some("a location's reference of a ref type"),
                    AliasValue::LocationAliasReference { .. } => Some("a location's reference"),
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
        .unwrap_or_else(|| "an alias the quest record does not list".into())
}

fn run() -> Result<(), E> {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let mut quest_id = None;
    if let Some(i) = a.iter().position(|x| x == "--quest") {
        quest_id = Some(
            a.get(i + 1)
                .cloned()
                .ok_or("--quest EDITOR_ID")?
                .to_ascii_lowercase(),
        );
        a.drain(i..i + 2);
    }
    if a.len() != 3 {
        return Err("fight_census MANIFEST OUT.txt NAMES.txt [--quest EDITOR_ID]".into());
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
        if !s.functions.values().any(|f| f.native()) {
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
    let fight_events: BTreeSet<String> = events
        .iter()
        .filter(|n| is_fight_event(n))
        .cloned()
        .collect();
    let fight_natives: BTreeSet<String> = natives
        .keys()
        .filter(|n| is_fight_native(n))
        .cloned()
        .collect();
    for (what, set) in [
        ("fight events", &fight_events),
        ("fight natives", &fight_natives),
    ] {
        *c.entry(format!("0 names read from the scripts\t{what}"))
            .or_default() += set.len() as u64;
        let _ = writeln!(names, "## {what}: {set:?}");
    }
    *c.entry("0 scripts read".into()).or_default() += lib.scripts.len() as u64;
    // 6: scripts declaring no auto state under an ancestor that declares
    // one (`pex_auto_state_inherited_v1`), and those among them whose
    // fight handlers stand only in that ancestor's auto state.
    for s in lib.scripts.values() {
        if !s.auto_state.is_empty() {
            continue;
        }
        let ch = chain(&lib, &s.name);
        let Some(anc) = ch
            .iter()
            .skip(1)
            .find(|t| !lib.scripts[*t].auto_state.is_empty())
        else {
            continue;
        };
        let auto = lib.scripts[anc].auto_state.clone();
        bump(
            &mut c,
            "6 scripts declaring no auto state whose nearest ancestor declaring one does",
        );
        let mut only_there: BTreeSet<&String> = BTreeSet::new();
        let mut elsewhere: BTreeSet<&String> = BTreeSet::new();
        for t in &ch {
            for ((st, n), f) in &lib.scripts[t].functions {
                if !fight_events.contains(n) || f.native() || f.code.is_empty() {
                    continue;
                }
                if *st == auto {
                    only_there.insert(n);
                } else {
                    elsewhere.insert(n);
                }
            }
        }
        for n in only_there.difference(&elsewhere) {
            bump(&mut c, format!("6 those scripts whose fight handler stands only in the ancestor's auto state\t{n}"));
            let _ = writeln!(
                names,
                "## {} inherits auto state {auto:?} from {anc} for {n}",
                s.name
            );
        }
    }
    // 1 and 2.
    let mut scripts_by_event: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for s in lib.scripts.values() {
        let ext = extends(&lib, &s.name);
        for ((state, n), f) in &s.functions {
            if f.native() || f.code.is_empty() || !fight_events.contains(n) {
                continue;
            }
            scripts_by_event
                .entry(n.clone())
                .or_default()
                .insert(s.name.clone());
            bump(&mut c, format!("1 fight handlers with code\t{n}"));
            bump(
                &mut c,
                format!("1 fight handlers with code by type extended\t{n}\t{ext}"),
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
                format!("1 fight handlers with code by state\t{n}\t{place}"),
            );
            let params: Vec<&str> = f.params.iter().map(|(_, t)| t.as_str()).collect();
            bump(
                &mut c,
                format!(
                    "1 fight handlers with code by parameter types\t{n}\t({})",
                    params.join(", ")
                ),
            );
            let _ = writeln!(
                names,
                "## handler {n} in {} state {state:?} ({ext})",
                s.name
            );
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
                    format!("2 calls in the fight handlers by name\t{n}\t{kind}"),
                );
                calls.insert(kind);
            }
            for k in calls {
                bump(
                    &mut c,
                    format!("2 fight handlers making at least one call of the name\t{n}\t{k}"),
                );
            }
        }
    }
    for (n, s) in &scripts_by_event {
        *c.entry(format!("1 scripts declaring a handler with code\t{n}"))
            .or_default() += s.len() as u64;
    }
    // 4: native call sites and registrations over every script.
    for s in lib.scripts.values() {
        for f in bodies(s) {
            for i in &f.code {
                let Some(name) = call_name(i) else { continue };
                if !fight_natives.contains(name) {
                    continue;
                }
                bump(
                    &mut c,
                    format!("4 fight native call sites over every script\t{name}"),
                );
                if name.starts_with("registerfor") {
                    let place = if f.name == "oninit" || f.name == "onaliasinit" {
                        "an init handler (OnInit, OnAliasInit)"
                    } else if fight_events.contains(&f.name) {
                        "a fight event's own handler"
                    } else {
                        "another function"
                    };
                    bump(&mut c, format!("4 fight event registrations by the function they stand in\t{name}\t{place}"));
                }
            }
        }
    }
    // 3 and 5: the records attaching them.
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
        let target = r.signature == *b"QUST"
            && quest_id
                .as_ref()
                .is_some_and(|p| bind::editor_id(&r).is_some_and(|e| e.to_ascii_lowercase() == *p));
        for x in &v.scripts {
            let script = fo4_pex::index::lower(x.name);
            let ev = cache
                .entry(script.clone())
                .or_insert_with(|| declared(&lib, &fight_events, &script))
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
            if target {
                bump(
                    &mut c,
                    format!(
                        "5 the quest: its own scripts\t{}",
                        if lib.scripts.contains_key(&script) {
                            "read"
                        } else {
                            "not in the library"
                        }
                    ),
                );
                let _ = writeln!(names, "## the quest attaches {script}");
            }
        }
        let Some(q) = &v.quest else { continue };
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
            let id = i64::from(al.object.alias);
            let fill = fill_kind(quest.as_ref(), id);
            let mut alias_events = BTreeSet::new();
            for x in &al.scripts {
                let script = fo4_pex::index::lower(x.name);
                let ev = cache
                    .entry(script.clone())
                    .or_insert_with(|| declared(&lib, &fight_events, &script))
                    .clone();
                for e in &ev {
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
                if !target || ev.is_empty() {
                    continue;
                }
                alias_events.extend(ev.iter().cloned());
                for e in &ev {
                    for t in chain(&lib, &script) {
                        for ((_, n), f) in &lib.scripts[&t].functions {
                            if n != e || f.native() || f.code.is_empty() {
                                continue;
                            }
                            let mut got = Reach::default();
                            let mut seen = BTreeSet::new();
                            reach(&lib, &t, f, 0, &mut seen, &mut got);
                            for (_, nat) in &got.natives {
                                bump(&mut c, format!("5 the quest: natives reached from its aliases' fight handlers\t{e}\t{nat}"));
                            }
                            for f2 in &got.code {
                                let _ = writeln!(
                                    names,
                                    "## the quest: alias {id} {e} in {t} reaches {f2}"
                                );
                            }
                            bump(
                                &mut c,
                                format!("5 the quest: functions with code reached from its aliases' fight handlers\t{e}\t{}", got.code.len()),
                            );
                            for o in &got.open {
                                bump(&mut c, format!("5 the quest: calls not resolved from its aliases' fight handlers\t{e}\t{o}"));
                            }
                        }
                    }
                }
            }
            if target && !alias_events.is_empty() {
                let evs: Vec<&str> = alias_events.iter().map(String::as_str).collect();
                bump(
                    &mut c,
                    format!(
                        "5 the quest: aliases with a fight handler, by events and fill\t{}\t{fill}",
                        evs.join(" ")
                    ),
                );
                let _ = writeln!(names, "## the quest's alias {id}: {evs:?} ({fill})");
            }
        }
    }
    let mut out = String::from(
        "# fight_census: kinds and counts only (spec/vm.md, The first fight). Sections: 0 names read from the scripts; 1 fight event handlers; 2 what they call; 3 the records attaching them;\n# 4 fight natives over every script and the event registrations; 5 the next main quest: its aliases with fight handlers and the calls those reach; 6 auto states declared only by an ancestor.\n",
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
