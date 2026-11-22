#![forbid(unsafe_code)]
//! Scripts that wait on a reference's 3D or its cell loading, and the
//! records that attach them (`spec/vm.md`, Load events):
//! `load_census MANIFEST OUT.txt NAMES.txt`.
//!
//! OUT.txt holds counts and kinds only (`spec/load-event-census.txt`);
//! NAMES.txt names scripts and records and is never versioned. Nothing
//! runs:
//!
//! 1. the handlers with code the compiled scripts declare whose name holds
//!    `load`, `attach` or `detach` (the names are read from the scripts,
//!    not from a list): by event, by the engine type the script extends
//!    (its first ancestor among [`TYPES`]), by whether they sit in the
//!    empty state, in the script's auto state or in another state;
//! 2. what those handlers call, by engine call name ([`CALLS`]; any other
//!    call counted as one kind);
//! 3. the attachments (one per record or alias and script) of a script
//!    declaring such a handler (its own or an ancestor's): on placed
//!    references by record type, cell kind, persistence and
//!    initially-disabled flag; on base records by record type; on quest
//!    aliases by the alias's fill;
//! 4. call sites of the natives that ask about 3D or wait for it.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{
    actor::Context,
    quest::{AliasValue, Quest},
    typed::{CellKind, Store},
    vmad::Vmad,
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

/// Engine call names counted one by one inside the handlers.
const CALLS: [&str; 24] = [
    "registerforanimationevent",
    "unregisterforanimationevent",
    "registerforremoteevent",
    "registerforcustomevent",
    "registerfordistancelessthanevent",
    "registerfordistancegreaterthanevent",
    "registerformenuopencloseevent",
    "starttimer",
    "starttimergametime",
    "gotostate",
    "setstage",
    "getstagedone",
    "getparentcell",
    "getcurrentlocation",
    "is3dloaded",
    "waitfor3dload",
    "isdisabled",
    "enable",
    "disable",
    "playanimation",
    "playidle",
    "evaluatepackage",
    "blockactivation",
    "moveto",
];

/// Natives on 3D counted over every script.
const NATIVES: [&str; 4] = ["is3dloaded", "waitfor3dload", "getparentcell", "isinterior"];

fn is_load_event(name: &str) -> bool {
    name.starts_with("on")
        && (name.contains("load") || name.contains("attach") || name.contains("detach"))
}

/// The engine type a script extends: itself or its first ancestor among
/// [`TYPES`].
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

/// The load events with code a script or its ancestors declare.
fn declared(lib: &MapLibrary, script: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut t = script.to_string();
    for _ in 0..fo4_vm::MAX_PARENTS {
        let Some(s) = lib.scripts.get(&t) else {
            break;
        };
        for ((_, n), f) in &s.functions {
            if is_load_event(n) && !f.native() && !f.code.is_empty() {
                out.insert(n.clone());
            }
        }
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

fn run() -> Result<(), E> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 3 {
        return Err("load_census MANIFEST OUT.txt NAMES.txt".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let lib = fo4_vm::setters::read_all_scripts(&vfs, m.limits.max_memory)?;
    let max = m.limits.max_memory;
    let mut c = Counts::new();
    let mut names = String::new();
    // 1 and 2: the handlers.
    *c.entry("1 scripts read".into()).or_default() += lib.scripts.len() as u64;
    let mut scripts_by_event: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for s in lib.scripts.values() {
        let ext = extends(&lib, &s.name);
        for ((state, n), f) in &s.functions {
            if !is_load_event(n) || f.native() || f.code.is_empty() {
                continue;
            }
            scripts_by_event
                .entry(n.clone())
                .or_default()
                .insert(s.name.clone());
            bump(&mut c, format!("1 handlers with code\t{n}"));
            bump(
                &mut c,
                format!("1 handlers with code by type extended\t{n}\t{ext}"),
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
                format!("1 handlers with code by state\t{n}\t{place}"),
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
                    format!("2 calls in the handlers by name\t{n}\t{kind}"),
                );
                calls.insert(kind);
            }
            for k in calls {
                bump(
                    &mut c,
                    format!("2 handlers making at least one call of the name\t{n}\t{k}"),
                );
            }
        }
    }
    for (n, s) in &scripts_by_event {
        *c.entry(format!("1 scripts declaring a handler with code\t{n}"))
            .or_default() += s.len() as u64;
    }
    // 4: natives on 3D over every script.
    for s in lib.scripts.values() {
        let mut fs: Vec<_> = s.functions.values().cloned().collect();
        for p in &s.properties {
            fs.extend(p.read.iter().cloned());
            fs.extend(p.write.iter().cloned());
        }
        for f in &fs {
            for i in &f.code {
                if let Some(name) = call_name(i).filter(|n| NATIVES.contains(n)) {
                    bump(&mut c, format!("4 call sites over every script\t{name}"));
                }
            }
        }
    }
    // 3: the records attaching them.
    let mut cache: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut events_of = |name: &str| -> BTreeSet<String> {
        cache
            .entry(name.to_string())
            .or_insert_with(|| declared(&lib, name))
            .clone()
    };
    let cell_of: BTreeMap<FormKey, (FormKey, u32)> = store
        .placement_groups()
        .filter(|(r, _, _)| !r.deleted)
        .map(|(r, cell, g)| (store.overrides().form_key(r.key, store.order()), (cell, g)))
        .collect();
    let winners: Vec<FormKey> = store
        .winners()
        .filter(|w| !w.deleted)
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .collect();
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
        for x in &v.scripts {
            let script = fo4_pex::index::lower(x.name);
            for e in events_of(&script) {
                match (sig.as_str(), cell_of.get(&k)) {
                    ("REFR" | "ACHR", Some((cell, group))) => {
                        let kind = match store.kind(cell) {
                            Some(CellKind::Interior) => "an interior cell",
                            Some(CellKind::Exterior) => "an exterior cell",
                            None => "a cell the store does not index",
                        };
                        let persistence = if *group == 8 {
                            "persistent"
                        } else {
                            "temporary"
                        };
                        let disabled = if r.flags & 0x800 != 0 {
                            "initially disabled"
                        } else {
                            "initially enabled"
                        };
                        bump(
                            &mut c,
                            format!(
                                "3 attachments on placed references of a script with the handler\t{e}\t{sig}\t{kind}\t{persistence}\t{disabled}"
                            ),
                        );
                    }
                    _ => bump(
                        &mut c,
                        format!("3 attachments on base or other records of a script with the handler\t{e}\t{sig}"),
                    ),
                }
                let _ = writeln!(names, "## {sig} {} attaches {script} ({e})", record_key(&k));
            }
        }
        if let Some(q) = &v.quest {
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
                    for e in events_of(&script) {
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
    let mut out = String::from(
        "# load_census: kinds and counts only (spec/vm.md, Load events). Sections: 1 handlers whose name holds load, attach or detach, by event, by the engine type extended and by state;\n# 2 what those handlers call; 3 the records whose VMAD attaches a script declaring one (placed references, base records, quest alias scripts by fill); 4 natives on 3D over every script.\n",
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
