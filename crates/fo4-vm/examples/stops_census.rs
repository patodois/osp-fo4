#![forbid(unsafe_code)]
//! What the data holds about the stops of the run past the museum
//! (`spec/vm.md`, Past the museum): `stops_census MANIFEST OUT.txt
//! NAMES.txt [--player-base EDITOR_ID] [--quest EDITOR_ID]`.
//!
//! OUT.txt holds counts and kinds only (`spec/quest-run-onward-census.txt`);
//! NAMES.txt names scripts and records and is never versioned. Nothing
//! runs. Sections:
//!
//! 1. terminal records (TERM): their menu items as stored (the
//!    subrecords from one `ITXT` to the next, read in order), the fragment
//!    block of their VMAD, which item each fragment's index names, and the
//!    placed references of terminal bases;
//! 2. calls of `Lock` over every script, by the literal of the first
//!    argument, and the handlers with code of `OnLockStateChanged`;
//! 3. the inventory events (event names holding `item` or `container`):
//!    handlers with code by type extended, and the call sites of the
//!    inventory event filter natives by the function they stand in and the
//!    declared type of their argument;
//! 4. perks: call sites of the perk natives, and the `PRKR` subrecords of
//!    actor bases by size (with `--player-base`, the player's base's
//!    count);
//! 5. actor values: call sites of `GetValue`, `GetBaseValue`, `SetValue`,
//!    `ModValue` by the engine type of the receiver (the nearest script of
//!    its declared type's chain that declares natives);
//! 6. reference aliases filled from another quest's alias (`ALEQ` then
//!    `ALEA`): how many, and how the alias they name is filled (with
//!    `--quest`, the quest's own);
//! 7. factions: call sites of the faction natives and of `AddToFaction`,
//!    the `SNAM` subrecords of actor bases by size and rank byte, and the
//!    bases naming a template (`TPLT`);
//! 8. dialogue lines (INFO) whose VMAD attaches scripts of their own (not
//!    the fragment block), by the engine type those scripts extend and the
//!    line events they handle with code (`OnBegin`, `OnEnd`).
use fo4_core::{Budget, Manifest};
use fo4_plugin::{
    actor::Context,
    quest::{AliasKind, AliasValue, Quest},
    typed::Store,
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
fn add(m: &mut Counts, k: impl Into<String>, n: u64) {
    *m.entry(k.into()).or_default() += n;
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

/// The nearest script of a type's chain that declares natives (an engine
/// script), so that no game script's name is counted.
fn engine(lib: &MapLibrary, t: &str) -> String {
    let mut x = t.to_string();
    for _ in 0..fo4_vm::MAX_PARENTS {
        let Some(s) = lib.scripts.get(&x) else {
            return x;
        };
        if s.functions.values().any(|f| f.native()) || s.parent.is_empty() {
            return x;
        }
        x = s.parent.clone();
    }
    "a chain too deep".into()
}

/// The engine type of an identifier inside a function of a script
/// ([`engine`] of its declared type): a local, a parameter, a script
/// variable, or `self`.
fn type_of(lib: &MapLibrary, script: &str, f: &fo4_vm::Func, id: &str) -> String {
    engine(lib, &declared_type(lib, script, f, id))
}

/// The declared type of an identifier inside a function of a script:
/// a local, a parameter, a script variable, or `self`.
fn declared_type(lib: &MapLibrary, script: &str, f: &fo4_vm::Func, id: &str) -> String {
    if id == "self" {
        return extends(lib, script);
    }
    if let Some((_, t)) = f.locals.iter().chain(&f.params).find(|(n, _)| n == id) {
        return t.clone();
    }
    let mut t = script.to_string();
    for _ in 0..fo4_vm::MAX_PARENTS {
        let Some(s) = lib.scripts.get(&t) else { break };
        if let Some(v) = s.variables.iter().find(|v| v.name == id) {
            return v.type_name.clone();
        }
        t = s.parent.clone();
    }
    "undeclared".into()
}

/// A method call's name, receiver and arguments (opcode 23).
fn method(i: &fo4_vm::Ins) -> Option<(&str, &Lit, &[Lit])> {
    match (i.op, i.args.first(), i.args.get(1)) {
        (23, Some(Lit::Ident(n)), Some(r)) => Some((n.as_str(), r, &i.extra)),
        _ => None,
    }
}

fn lit_kind(l: Option<&Lit>) -> &'static str {
    match l {
        Some(Lit::Bool(true)) => "the literal true",
        Some(Lit::Bool(false)) => "the literal false",
        Some(Lit::None) => "none",
        Some(Lit::Ident(_)) => "a variable",
        Some(_) => "another literal",
        None => "no argument",
    }
}

/// One menu item of a terminal as stored.
#[derive(Default)]
struct Item {
    id: Option<u16>,
    kind: Option<u8>,
    conditions: usize,
    submenu: bool,
    other: BTreeSet<String>,
}

/// The winning record of a type and an editor ID.
fn by_editor_id(store: &Store, max: u64, sig: &[u8; 4], edid: &str) -> Option<FormKey> {
    store
        .winners()
        .filter(|w| !w.deleted && w.signature == *sig)
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .find(|k| {
            store
                .raw(k, &Budget::new(max))
                .ok()
                .flatten()
                .and_then(|r| fo4_vm::bind::editor_id(&r))
                .is_some_and(|e| e.eq_ignore_ascii_case(edid))
        })
}

fn run() -> Result<(), E> {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let mut player_base = None;
    let mut quest_id = None;
    for (flag, slot) in [
        ("--player-base", &mut player_base),
        ("--quest", &mut quest_id),
    ] {
        if let Some(i) = a.iter().position(|x| x == flag) {
            *slot = Some(a.get(i + 1).cloned().ok_or("a flag without its value")?);
            a.drain(i..i + 2);
        }
    }
    if a.len() != 3 {
        return Err(
            "stops_census MANIFEST OUT.txt NAMES.txt [--player-base EDITOR_ID] [--quest EDITOR_ID]"
                .into(),
        );
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let lib = fo4_vm::setters::read_all_scripts(&vfs, m.limits.max_memory)?;
    let max = m.limits.max_memory;
    let mut c = Counts::new();
    let mut names = String::new();

    // The winning records by type.
    let mut by_sig: BTreeMap<[u8; 4], Vec<FormKey>> = BTreeMap::new();
    for o in store.winners() {
        if matches!(&o.signature, b"TERM" | b"QUST" | b"NPC_" | b"INFO") {
            by_sig
                .entry(o.signature)
                .or_default()
                .push(store.overrides().form_key(o.key, store.order()));
        }
    }

    // 1 terminals.
    let terms = by_sig.get(b"TERM").cloned().unwrap_or_default();
    add(&mut c, "1 terminal records (TERM)", terms.len() as u64);
    let mut with_fragments: BTreeSet<FormKey> = BTreeSet::new();
    for k in &terms {
        let b = Budget::new(max);
        let Some(r) = store.raw(k, &b)? else { continue };
        let mut items: Vec<Item> = Vec::new();
        let mut declared = None;
        let mut fragments: Vec<(u32, String, String)> = Vec::new();
        let mut block_script = String::new();
        for s in &r.subrecords {
            let d = r.data(s);
            match &s.signature {
                b"VMAD" => {
                    bump(&mut c, "1 terminal records with a VMAD");
                    let v = Vmad::read(d, r.signature, &r.source.file, s.offset, &b)?;
                    if !v.scripts.is_empty() {
                        bump(&mut c, "1 terminal records whose VMAD attaches scripts");
                    }
                    if let Some(t) = &v.terminal {
                        block_script = fo4_pex::index::lower(t.script.name);
                        for f in &t.fragments {
                            fragments.push((
                                f.index,
                                fo4_pex::index::lower(f.script),
                                fo4_pex::index::lower(f.function),
                            ));
                        }
                    }
                }
                b"ISIZ" if d.len() == 4 => {
                    declared = Some(u32::from_le_bytes(d.try_into()?));
                }
                b"ITXT" => items.push(Item::default()),
                sig => {
                    let Some(it) = items.last_mut() else { continue };
                    match sig {
                        b"ITID" if d.len() == 2 => it.id = Some(u16::from_le_bytes(d.try_into()?)),
                        b"ANAM" if d.len() == 1 => it.kind = Some(d[0]),
                        b"CTDA" => it.conditions += 1,
                        b"TNAM" => it.submenu = true,
                        other => {
                            it.other.insert(String::from_utf8_lossy(other).into_owned());
                        }
                    }
                }
            }
        }
        add(&mut c, "1 menu items (ITXT)", items.len() as u64);
        if declared.is_some_and(|n| n as usize != items.len()) {
            bump(
                &mut c,
                "1 terminal records whose ISIZ differs from their ITXT count",
            );
        }
        let ids: Vec<u16> = items.iter().filter_map(|i| i.id).collect();
        let distinct: BTreeSet<u16> = ids.iter().copied().collect();
        if distinct.len() != ids.len() {
            bump(&mut c, "1 terminal records with an item id stored twice");
        }
        for it in &items {
            bump(
                &mut c,
                format!(
                    "1 menu items by their ANAM byte\t{}",
                    it.kind.map(|x| x.to_string()).unwrap_or("none".into())
                ),
            );
            if it.id.is_none() {
                bump(&mut c, "1 menu items without an ITID");
            }
            if it.conditions > 0 {
                bump(&mut c, "1 menu items with conditions");
                add(&mut c, "1 conditions of menu items", it.conditions as u64);
            }
            if it.submenu {
                bump(&mut c, "1 menu items with a TNAM (another terminal)");
            }
            for o in &it.other {
                bump(
                    &mut c,
                    format!("1 menu items holding another subrecord\t{o}"),
                );
            }
        }
        if fragments.is_empty() {
            continue;
        }
        with_fragments.insert(k.clone());
        bump(&mut c, "1 terminal records with a fragment block");
        for (index, script, function) in &fragments {
            bump(&mut c, "1 fragments");
            if *script != block_script {
                bump(&mut c, "1 fragments naming a script other than the block's");
            }
            let item = items
                .iter()
                .find(|i| i.id.is_some_and(|x| u32::from(x) == *index));
            match item {
                Some(i) => {
                    bump(&mut c, "1 fragments whose index is an item's ITID");
                    if i.conditions > 0 {
                        bump(&mut c, "1 fragments whose item has conditions");
                    }
                    if i.submenu {
                        bump(&mut c, "1 fragments whose item opens another terminal");
                    }
                }
                None => bump(&mut c, "1 fragments whose index is no item's ITID"),
            }
            let f = lib
                .scripts
                .get(script)
                .and_then(|s| s.functions.iter().find(|((_, n), _)| n == function))
                .map(|(_, f)| f.clone());
            match f {
                None => bump(&mut c, "1 fragments whose function the scripts do not hold"),
                Some(f) => {
                    let params: Vec<&str> = f.params.iter().map(|(_, t)| t.as_str()).collect();
                    bump(
                        &mut c,
                        format!(
                            "1 fragment functions by parameter types\t({})",
                            params.join(", ")
                        ),
                    );
                    let calls: BTreeSet<&str> = f
                        .code
                        .iter()
                        .filter_map(|i| method(i).map(|(n, _, _)| n))
                        .collect();
                    for n in calls {
                        if matches!(
                            n,
                            "setstage"
                                | "lock"
                                | "unlock"
                                | "setopen"
                                | "additem"
                                | "enable"
                                | "disable"
                        ) {
                            bump(&mut c, format!("1 fragment functions calling\t{n}"));
                        }
                    }
                    let _ = writeln!(
                        names,
                        "## terminal {} item {index}: {script}.{function}",
                        record_key(k)
                    );
                }
            }
        }
    }
    // Placed references of terminal bases.
    let term_set: BTreeSet<FormKey> = terms.iter().cloned().collect();
    for (o, _cell, _g) in store.placement_groups() {
        if o.signature != *b"REFR" {
            continue;
        }
        let k = store.overrides().form_key(o.key, store.order());
        let b = Budget::new(max);
        let Some((r, plugin)) = store.raw_with_plugin(&k, &b)? else {
            continue;
        };
        let Some(s) = r.subrecords.iter().find(|s| s.signature == *b"NAME") else {
            continue;
        };
        let d = r.data(s);
        if d.len() < 4 {
            continue;
        }
        let id = u32::from_le_bytes(d[..4].try_into()?);
        let Some(base) = store.order().remap(plugin, id, &r.source.file, 0)? else {
            continue;
        };
        if term_set.contains(&base) {
            bump(&mut c, "1 placed references of terminal bases");
            if with_fragments.contains(&base) {
                bump(
                    &mut c,
                    "1 placed references of terminal bases with a fragment block",
                );
            }
        }
    }

    // 2 to 5: call sites and handlers over every script.
    let perk_natives = ["hasperk", "addperk", "removeperk"];
    let faction_natives = [
        "isinfaction",
        "setfactionrank",
        "removefromfaction",
        "removefromallfactions",
        "addtofaction",
    ];
    let value_natives = ["getvalue", "getbasevalue", "setvalue", "modvalue"];
    let filter_natives = [
        "addinventoryeventfilter",
        "removeinventoryeventfilter",
        "removeallinventoryeventfilters",
    ];
    let mut inventory_events: BTreeSet<String> = BTreeSet::new();
    for s in lib.scripts.values() {
        if !s.functions.values().any(|f| f.native()) {
            continue;
        }
        for ((_, n), f) in &s.functions {
            if !f.native()
                && f.code.is_empty()
                && n.starts_with("on")
                && (n.contains("item") || n.contains("container"))
            {
                inventory_events.insert(n.clone());
            }
        }
    }
    let _ = writeln!(
        names,
        "## inventory events read from the scripts: {inventory_events:?}"
    );
    add(
        &mut c,
        "3 inventory event names read from the scripts",
        inventory_events.len() as u64,
    );
    let mut handler_scripts: BTreeSet<String> = BTreeSet::new();
    let mut filter_scripts: BTreeSet<String> = BTreeSet::new();
    for s in lib.scripts.values() {
        let ext = extends(&lib, &s.name);
        for ((_, n), f) in &s.functions {
            if f.native() || f.code.is_empty() {
                continue;
            }
            if n == "onlockstatechanged" {
                bump(
                    &mut c,
                    format!("2 OnLockStateChanged handlers with code by type extended\t{ext}"),
                );
            }
            if inventory_events.contains(n) {
                bump(
                    &mut c,
                    format!("3 inventory handlers with code by type extended\t{n}\t{ext}"),
                );
                if n == "onitemadded" || n == "onitemremoved" {
                    handler_scripts.insert(s.name.clone());
                }
            }
            for i in &f.code {
                let Some((name, recv, args)) = method(i) else {
                    continue;
                };
                if name == "lock" {
                    bump(
                        &mut c,
                        format!(
                            "2 Lock call sites by the first argument\t{}",
                            lit_kind(args.first())
                        ),
                    );
                    if n == "onlockstatechanged" {
                        bump(
                            &mut c,
                            "2 Lock call sites inside an OnLockStateChanged handler",
                        );
                    }
                }
                if filter_natives.contains(&name) {
                    filter_scripts.insert(s.name.clone());
                    let place = match n.as_str() {
                        "oninit" | "onaliasinit" => "an init handler",
                        "onload" => "OnLoad",
                        "oneffectstart" => "OnEffectStart",
                        x if inventory_events.contains(x) => "an inventory handler",
                        _ => "another function",
                    };
                    bump(&mut c, format!("3 inventory filter call sites by the function they stand in\t{name}\t{place}"));
                    if name == "addinventoryeventfilter" {
                        let ty = match args.first() {
                            Some(Lit::Ident(id)) => type_of(&lib, &s.name, f, id),
                            Some(Lit::None) => "none".into(),
                            _ => "another argument".into(),
                        };
                        bump(&mut c, format!("3 AddInventoryEventFilter call sites by the argument's engine type\t{ty}"));
                        let on_self = matches!(recv, Lit::Ident(r) if r == "self");
                        bump(
                            &mut c,
                            format!(
                                "3 AddInventoryEventFilter call sites by receiver\t{}",
                                if on_self { "self" } else { "another object" }
                            ),
                        );
                    }
                }
                if perk_natives.contains(&name) {
                    let ty = match recv {
                        Lit::Ident(r) => type_of(&lib, &s.name, f, r),
                        _ => "another receiver".into(),
                    };
                    bump(
                        &mut c,
                        format!(
                            "4 perk native call sites by the receiver's engine type\t{name}\t{ty}"
                        ),
                    );
                }
                if faction_natives.contains(&name) {
                    bump(&mut c, format!("7 faction call sites\t{name}"));
                }
                if value_natives.contains(&name) {
                    let ty = match recv {
                        Lit::Ident(r) => type_of(&lib, &s.name, f, r),
                        _ => "another receiver".into(),
                    };
                    bump(
                        &mut c,
                        format!("5 value call sites by the receiver's engine type\t{name}\t{ty}"),
                    );
                }
            }
        }
    }
    add(
        &mut c,
        "3 scripts declaring an OnItemAdded or OnItemRemoved handler with code",
        handler_scripts.len() as u64,
    );
    let mut with_filter = 0;
    for h in &handler_scripts {
        let mut t = h.clone();
        let mut found = false;
        for _ in 0..fo4_vm::MAX_PARENTS {
            if filter_scripts.contains(&t) {
                found = true;
                break;
            }
            let Some(s) = lib.scripts.get(&t) else { break };
            t = s.parent.clone();
        }
        with_filter += u64::from(found);
    }
    add(
        &mut c,
        "3 those scripts whose chain calls an inventory filter native",
        with_filter,
    );
    // The perk natives' declaring scripts and the script functions calling
    // them (IsInPowerArmor and the like read a perk).
    for nat in perk_natives {
        let decl: Vec<&str> = lib
            .scripts
            .values()
            .filter(|s| s.functions.iter().any(|((_, n), f)| n == nat && f.native()))
            .map(|s| s.name.as_str())
            .collect();
        let _ = writeln!(names, "## native {nat} declared by {decl:?}");
    }
    for s in ["actor", "objectreference"] {
        let Some(sc) = lib.scripts.get(s) else {
            continue;
        };
        for ((_, n), f) in &sc.functions {
            if f.native() || f.code.is_empty() {
                continue;
            }
            if f.code
                .iter()
                .any(|i| method(i).is_some_and(|(x, _, _)| x == "hasperk"))
            {
                bump(
                    &mut c,
                    format!("4 functions with code of the engine scripts calling HasPerk\t{s}"),
                );
                let _ = writeln!(names, "## {s}.{n} calls hasperk");
            }
        }
    }
    // 4 PRKR of actor bases.
    let npcs = by_sig.get(b"NPC_").cloned().unwrap_or_default();
    let player_key = match &player_base {
        Some(e) => by_editor_id(&store, max, b"NPC_", e),
        None => None,
    };
    for k in &npcs {
        let b = Budget::new(max);
        let Some(r) = store.raw(k, &b)? else { continue };
        let prkr: Vec<usize> = r
            .subrecords
            .iter()
            .filter(|s| s.signature == *b"PRKR")
            .map(|s| r.data(s).len())
            .collect();
        let snam: Vec<&[u8]> = r
            .subrecords
            .iter()
            .filter(|s| s.signature == *b"SNAM")
            .map(|s| r.data(s))
            .collect();
        if r.subrecords.iter().any(|s| s.signature == *b"TPLT") {
            bump(&mut c, "7 actor bases naming a template (TPLT)");
        }
        if !snam.is_empty() {
            bump(&mut c, "7 actor bases with SNAM");
        }
        for d in &snam {
            let rank = match d.len() {
                5 if (d[4] as i8) < 0 => "a negative rank byte",
                5 => "a rank byte of 0 or more",
                _ => "another size",
            };
            bump(
                &mut c,
                format!("7 SNAM subrecords by size and rank\t{}\t{rank}", d.len()),
            );
        }
        if player_key.as_ref() == Some(k) {
            add(
                &mut c,
                "7 SNAM subrecords of the player's base",
                snam.len() as u64,
            );
        }
        if prkr.is_empty() {
            continue;
        }
        bump(&mut c, "4 actor bases (NPC_) with PRKR");
        for n in &prkr {
            bump(&mut c, format!("4 PRKR subrecords by size\t{n}"));
        }
        if player_key.as_ref() == Some(k) {
            add(
                &mut c,
                "4 PRKR subrecords of the player's base",
                prkr.len() as u64,
            );
        }
    }

    // 6 aliases filled from another quest's alias.
    let quests = by_sig.get(b"QUST").cloned().unwrap_or_default();
    let mut read: BTreeMap<FormKey, Quest> = BTreeMap::new();
    let b = Budget::new(max);
    for k in &quests {
        let Some((r, plugin)) = store.raw_with_plugin(k, &b)? else {
            continue;
        };
        let q = Quest::read(
            &r,
            Context {
                order: store.order(),
                plugin,
            },
            &b,
        )?;
        read.insert(k.clone(), q);
    }
    let fill_kind = |q: &Quest, id: i32| -> String {
        let Some(al) = q.aliases.iter().find(|x| x.id as i32 == id) else {
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
                _ => None,
            })
            .collect();
        kinds.dedup();
        if kinds.is_empty() {
            "another fill (conditions, a list or none)".into()
        } else {
            kinds.join(" and ")
        }
    };
    let target_quest = quest_id
        .as_ref()
        .and_then(|e| by_editor_id(&store, max, b"QUST", e));
    for (k, q) in &read {
        for al in &q.aliases {
            for v in &al.values {
                let AliasValue::External { quest, alias } = v else {
                    continue;
                };
                bump(&mut c, "6 aliases filled from another quest's alias");
                let what = match quest.key.as_ref().and_then(|x| read.get(x)) {
                    Some(other) => fill_kind(other, *alias),
                    None => "a quest the load order does not hold".into(),
                };
                bump(
                    &mut c,
                    format!("6 those aliases by how the alias they name is filled\t{what}"),
                );
                if quest.key.as_ref() == Some(k) {
                    bump(&mut c, "6 those aliases naming their own quest");
                }
                if target_quest.as_ref() == Some(k) {
                    bump(&mut c, format!("6 the quest: its aliases filled from another quest's alias, by how that alias is filled\t{what}"));
                    let _ = writeln!(
                        names,
                        "## the quest's alias {} names alias {alias} of {}",
                        al.id,
                        quest.key.as_ref().map(record_key).unwrap_or_default()
                    );
                }
            }
        }
    }

    // 8 dialogue lines' own scripts.
    for k in by_sig.get(b"INFO").cloned().unwrap_or_default() {
        let b = Budget::new(max);
        let Some(r) = store.raw(&k, &b)? else {
            continue;
        };
        let Some(s) = r.subrecords.iter().find(|s| s.signature == *b"VMAD") else {
            continue;
        };
        let v = Vmad::read(r.data(s), r.signature, &r.source.file, s.offset, &b)?;
        if v.scripts.is_empty() {
            continue;
        }
        bump(
            &mut c,
            "8 dialogue lines whose VMAD attaches scripts of their own",
        );
        for sc in &v.scripts {
            let name = fo4_pex::index::lower(sc.name);
            bump(
                &mut c,
                format!("8 those scripts by engine type\t{}", engine(&lib, &name)),
            );
            for ev in ["onbegin", "onend"] {
                let mut t = name.clone();
                let mut found = false;
                for _ in 0..fo4_vm::MAX_PARENTS {
                    let Some(x) = lib.scripts.get(&t) else { break };
                    if x.functions
                        .iter()
                        .any(|((_, n), f)| n == ev && !f.native() && !f.code.is_empty())
                    {
                        found = true;
                        break;
                    }
                    t = x.parent.clone();
                }
                if found {
                    bump(
                        &mut c,
                        format!("8 those scripts handling a line event with code\t{ev}"),
                    );
                }
            }
        }
    }

    let mut out = String::from(
        "# stops_census: kinds and counts only (spec/vm.md, Past the museum). Sections: 1 terminals: menu items, fragments, placed terminals; 2 Lock calls and lock\n# state handlers; 3 inventory events and their filters; 4 perks; 5 actor values; 6 aliases filled from another quest's alias; 7 factions; 8 dialogue lines'\n# own scripts.\n",
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
