#![forbid(unsafe_code)]
//! Scene phase conditions that read script variables, and who writes those
//! variables (`spec/vm.md`, Scenes offered again when a script variable is
//! written): `scene_variable_census MANIFEST OUT.txt NAMES.txt`.
//!
//! OUT.txt holds counts and kinds only (`spec/scene-variable-census.txt`);
//! NAMES.txt names scenes, quests, scripts and variables and is never
//! versioned. Nothing runs:
//!
//! 1. every winning scene's phases: their start and completion lists, the
//!    conditions of function 629 (a quest script variable, CIS2 names it)
//!    and of function 660 (the other function carrying a CIS2 text; not
//!    evaluated by the interpreter), the phases and scenes holding them;
//! 2. the function 629 conditions of scene phases: the quest their first
//!    parameter names (the scene's owning quest or another), the operator,
//!    the comparison value, the shape of the CIS2 text, and whether the
//!    scripts the quest's VMAD attaches (its own and its stage fragment
//!    script, with their parents) declare the variable, and its type;
//! 3. the distinct (quest, variable) pairs those conditions read, and the
//!    instructions that write each variable: in the quest's own scripts
//!    (an instruction whose destination names the variable, or a property
//!    write on `self` of the property it backs), by the kind of function
//!    (an engine event handler by its name, a remote event handler, a
//!    stage fragment, another function, a property accessor); and in any
//!    other script, a property write of that property on a receiver whose
//!    declared type is the quest's script or extends it;
//! 4. the conditions of section 2 by the kinds of writer their variable
//!    has: what a run must do for the condition's value to change.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{actor::Context, condition::Comparison, typed::Store, FormKey};
use fo4_vfs::Vfs;
use fo4_vm::{
    bind::record_key,
    scene::SceneInfo,
    setters::{self, Attachment, Part},
    Func, Lit, MapLibrary,
};
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

/// Function 629: a quest script variable (`cond_fn_629_quest_variable_v1`).
const QUEST_VARIABLE: u16 = 629;
/// Function 660: the other function whose conditions carry a CIS2 text
/// (`spec/conditions.md`); not evaluated.
const OTHER_TEXT: u16 = 660;

/// The destination slot of the opcodes that write one (as `vm_checks`
/// reads them; `fo4_vm::setters` keeps the same table).
fn dest(op: u8) -> Option<usize> {
    match op {
        1..=19 | 27 | 30 | 31 | 32 | 36 | 37 | 38 => Some(0),
        23 | 25 | 28 => Some(2),
        24 | 34 | 40 => Some(1),
        _ => None,
    }
}

fn ident(l: &Lit) -> Option<&str> {
    match l {
        Lit::Ident(n) => Some(n),
        _ => None,
    }
}

/// The scripts of a chain, most derived first.
fn chain(lib: &MapLibrary, name: &str) -> Vec<Rc<fo4_vm::Script>> {
    let mut out = Vec::new();
    let mut at = name.to_string();
    for _ in 0..fo4_vm::MAX_PARENTS {
        let Some(s) = lib.scripts.get(&at) else { break };
        out.push(s.clone());
        if s.parent.is_empty() {
            break;
        }
        at = s.parent.clone();
    }
    out
}

fn decl_type(lib: &MapLibrary, f: &Func, l: &Lit) -> Option<String> {
    let n = ident(l)?;
    if n == "self" {
        return Some(f.script.clone());
    }
    if let Some((_, t)) = f.params.iter().chain(&f.locals).find(|(p, _)| p == n) {
        return Some(t.clone());
    }
    chain(lib, &f.script)
        .iter()
        .find_map(|s| s.variable(n).map(|v| v.type_name.clone()))
}

/// Every function with code of a script, with whether it is a property
/// accessor.
fn bodies(s: &fo4_vm::Script) -> Vec<(Rc<Func>, bool)> {
    let mut fs: Vec<(Rc<Func>, bool)> = s.functions.values().map(|f| (f.clone(), false)).collect();
    for p in &s.properties {
        fs.extend(p.read.iter().map(|f| (f.clone(), true)));
        fs.extend(p.write.iter().map(|f| (f.clone(), true)));
    }
    fs
}

/// What kind of function a writer is.
fn function_kind(f: &Func, accessor: bool, events: &BTreeSet<String>, fragment: bool) -> String {
    if accessor {
        return "a property accessor".into();
    }
    if events.contains(&f.name) {
        return format!("an engine event handler {}", f.name);
    }
    if f.name.starts_with("::remote_") {
        return "a remote or custom event handler".into();
    }
    if fragment {
        return "a stage fragment".into();
    }
    "another function of the quest's scripts".into()
}

/// The quest's scripts its VMAD attaches (own and stage fragment), lower
/// case, with whether each is the fragment script.
fn quest_scripts(
    by_quest: &BTreeMap<String, Vec<&Attachment>>,
    q: &FormKey,
) -> Vec<(String, bool)> {
    by_quest
        .get(&record_key(q))
        .into_iter()
        .flatten()
        .filter(|a| matches!(a.part, Part::Own | Part::Fragment))
        .map(|a| (a.script.clone(), a.part == Part::Fragment))
        .collect()
}

fn run() -> Result<(), E> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 3 {
        return Err("scene_variable_census MANIFEST OUT.txt NAMES.txt".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let lib = setters::read_all_scripts(&vfs, m.limits.max_memory)?;
    let max = m.limits.max_memory;
    let owners = setters::owners(&store);
    let (atts, _) = setters::attachments(&store, &owners, &Budget::new(max))?;
    let mut by_quest: BTreeMap<String, Vec<&Attachment>> = BTreeMap::new();
    for x in &atts {
        if x.signature == *b"QUST" {
            by_quest.entry(record_key(&x.record)).or_default().push(x);
        }
    }
    let events = setters::engine_events(&lib);
    let mut c = Counts::new();
    let mut names = String::new();
    for k in [
        "1 scenes holding a condition of function 629 in a phase list",
        "1 scenes holding a condition of function 660 in a phase list",
    ] {
        c.entry(k.into()).or_default();
    }
    // 1 and 2: the scenes.
    let scenes: Vec<FormKey> = store
        .winners()
        .filter(|w| !w.deleted && w.signature == *b"SCEN")
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .collect();
    // (quest, variable) -> the conditions reading it: (scene, phase, list).
    let mut pairs: BTreeMap<(String, String), (FormKey, Vec<String>)> = BTreeMap::new();
    let _ = writeln!(
        names,
        "# function 629 conditions of scene phases: scene, phase, list, quest, variable, operator, value"
    );
    for k in &scenes {
        let b = Budget::new(max);
        let Ok(Some(info)) = SceneInfo::load(&store, k, &b) else {
            bump(&mut c, "1 scenes not read");
            continue;
        };
        bump(&mut c, "1 scenes read");
        let ctx = Context {
            order: store.order(),
            plugin: info.plugin,
        };
        let mut has = [false; 2];
        for (p, ph) in info.phases.iter().enumerate() {
            bump(&mut c, "1 phases");
            for (list, conds) in [("start", &ph.start), ("completion", &ph.completion)] {
                let mut here = [false; 2];
                for cd in conds.iter() {
                    let f = cd.ctda.function;
                    let class = match f {
                        QUEST_VARIABLE => "629",
                        OTHER_TEXT => "660",
                        _ => "another function",
                    };
                    bump(&mut c, format!("1 phase conditions\t{list}\t{class}"));
                    if f == OTHER_TEXT {
                        here[1] = true;
                        continue;
                    }
                    if f != QUEST_VARIABLE {
                        continue;
                    }
                    here[0] = true;
                    let sec = "2 function 629 conditions of scene phases";
                    bump(&mut c, format!("{sec}\tby list\t{list}"));
                    bump(
                        &mut c,
                        format!("{sec}\toperator code\t{}", cd.ctda.operator()),
                    );
                    let value = match cd.ctda.comparison() {
                        Comparison::Global(_) => "a global".to_string(),
                        Comparison::Float(v) if v == 0.0 || v == 1.0 => format!("{v}"),
                        Comparison::Float(v) if v.fract() == 0.0 && (2.0..=9.0).contains(&v) => {
                            "an integer from 2 to 9".to_string()
                        }
                        Comparison::Float(v) if v.fract() == 0.0 && v >= 10.0 => {
                            "an integer of 10 or more".to_string()
                        }
                        Comparison::Float(_) => "another number".to_string(),
                    };
                    bump(&mut c, format!("{sec}\tcomparison value\t{value}"));
                    let text = cd
                        .cis2
                        .as_deref()
                        .map(fo4_pex::index::lower)
                        .unwrap_or_default();
                    let shape = if text.is_empty() {
                        "no CIS2"
                    } else if text.starts_with("::") && text.ends_with("_var") {
                        "begins with :: and ends with _var"
                    } else {
                        "other"
                    };
                    bump(&mut c, format!("{sec}\tCIS2 text shape\t{shape}"));
                    let quest = fo4_plugin::condition::form(ctx, cd.ctda.param1, &info.file, 0)
                        .ok()
                        .flatten()
                        .filter(|q| store.signature(q) == Some(*b"QUST"));
                    let Some(q) = quest else {
                        bump(&mut c, format!("{sec}\tquest\tno winning quest"));
                        continue;
                    };
                    let own = info.quest.as_ref() == Some(&q);
                    bump(
                        &mut c,
                        format!(
                            "{sec}\tquest\t{}",
                            if own {
                                "the scene's owning quest"
                            } else {
                                "another quest"
                            }
                        ),
                    );
                    let scripts = quest_scripts(&by_quest, &q);
                    let decl = scripts
                        .iter()
                        .flat_map(|(s, _)| chain(&lib, s))
                        .find_map(|s| {
                            s.variable(&text).map(|v| {
                                let backed = s
                                    .properties
                                    .iter()
                                    .any(|p| p.variable.as_deref() == Some(text.as_str()));
                                (v.type_name.clone(), backed)
                            })
                        });
                    let d = match (&decl, scripts.is_empty()) {
                        (_, true) => "the quest's VMAD attaches no script".to_string(),
                        (None, false) => "no attached script declares it".to_string(),
                        (Some((t, true)), _) => format!("declared, {t}, behind a property"),
                        (Some((t, false)), _) => format!("declared, {t}, no property"),
                    };
                    bump(&mut c, format!("{sec}\tthe variable\t{d}"));
                    let _ = writeln!(
                        names,
                        "{}\t{p}\t{list}\t{}\t{text}\t{}\t{value}",
                        record_key(k),
                        record_key(&q),
                        cd.ctda.operator()
                    );
                    pairs
                        .entry((record_key(&q), text.clone()))
                        .or_insert_with(|| (q.clone(), Vec::new()))
                        .1
                        .push(format!("{} phase {p} {list}", record_key(k)));
                }
                for (i, h) in here.iter().enumerate() {
                    if *h {
                        has[i] = true;
                        bump(
                            &mut c,
                            format!(
                                "1 phases whose {list} list holds a condition of function {}",
                                if i == 0 { 629 } else { 660 }
                            ),
                        );
                    }
                }
            }
        }
        for (i, h) in has.iter().enumerate() {
            if *h {
                bump(
                    &mut c,
                    format!(
                        "1 scenes holding a condition of function {} in a phase list",
                        if i == 0 { 629 } else { 660 }
                    ),
                );
            }
        }
    }
    // 3: the writers of each pair.
    *c.entry("3 distinct (quest, variable) pairs read by scene phase conditions".into())
        .or_default() += pairs.len() as u64;
    // Property writes (opcode 29) in every script, by property name.
    let mut prop_sets: BTreeMap<String, Vec<(Rc<Func>, usize)>> = BTreeMap::new();
    for s in lib.scripts.values() {
        for (f, _) in bodies(s) {
            for (at, ins) in f.code.iter().enumerate() {
                if ins.op == 29 {
                    if let Some(n) = ins.args.first().and_then(ident) {
                        prop_sets
                            .entry(n.to_string())
                            .or_default()
                            .push((f.clone(), at));
                    }
                }
            }
        }
    }
    let _ = writeln!(
        names,
        "# (quest, variable) pairs: quest, variable, writers (kind: script.function)"
    );
    let mut writer_kinds: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for ((qk, var), (q, readers)) in &pairs {
        let scripts = quest_scripts(&by_quest, q);
        let mut kinds: BTreeSet<String> = BTreeSet::new();
        let mut sites: Vec<String> = Vec::new();
        // The quest's scripts with their parents, and the property the
        // variable backs.
        let mut quest_script_names: BTreeSet<String> = BTreeSet::new();
        let mut prop: Option<String> = None;
        for (sname, _) in &scripts {
            for s in chain(&lib, sname) {
                quest_script_names.insert(s.name.clone());
                if prop.is_none() {
                    prop = s
                        .properties
                        .iter()
                        .find(|p| p.variable.as_deref() == Some(var.as_str()))
                        .map(|p| p.name.clone());
                }
            }
        }
        // A property write on a receiver of one of the quest's script
        // types (not the engine's quest type).
        let on_quest = |f: &Func, recv: &Lit| {
            let t = decl_type(&lib, f, recv).unwrap_or_default();
            t != setters::QUEST && quest_script_names.contains(&t)
        };
        let mut scanned: BTreeSet<String> = BTreeSet::new();
        for (sname, fragment) in &scripts {
            for s in chain(&lib, sname) {
                if !scanned.insert(s.name.clone()) {
                    continue;
                }
                for (f, accessor) in bodies(&s) {
                    for ins in &f.code {
                        let by_dest = dest(ins.op)
                            .and_then(|d| ins.args.get(d))
                            .and_then(ident)
                            .is_some_and(|d| d == var);
                        let by_prop = ins.op == 29
                            && prop.is_some()
                            && ins.args.first().and_then(ident) == prop.as_deref()
                            && ins.args.get(1).is_some_and(|r| on_quest(&f, r));
                        if by_dest || by_prop {
                            let kind = function_kind(&f, accessor, &events, *fragment);
                            sites.push(format!("{kind}: {}.{}", f.script, f.name));
                            kinds.insert(kind);
                        }
                    }
                }
            }
        }
        // Property writes from other scripts on a receiver of the quest's
        // script type.
        if let Some(p) = &prop {
            for (f, at) in prop_sets.get(p).into_iter().flatten() {
                if scanned.contains(&f.script) {
                    continue;
                }
                let recv = f.code[*at].args.get(1).cloned().unwrap_or(Lit::None);
                if on_quest(f, &recv) {
                    let kind = "a property write from another script".to_string();
                    sites.push(format!("{kind}: {}.{}", f.script, f.name));
                    kinds.insert(kind);
                }
            }
        }
        if kinds.is_empty() {
            kinds.insert("no writer found".into());
        }
        for kd in &kinds {
            bump(&mut c, format!("3 pairs with a writer of kind\t{kd}"));
        }
        bump(
            &mut c,
            format!("3 pairs by number of writer kinds\t{}", kinds.len()),
        );
        let _ = writeln!(names, "{qk}\t{var}\t{} reader(s)\t{sites:?}", readers.len());
        writer_kinds.insert((qk.clone(), var.clone()), kinds);
    }
    // 4: the conditions by their variable's writer kinds.
    for ((qk, var), (_, readers)) in &pairs {
        let kinds = &writer_kinds[&(qk.clone(), var.clone())];
        let event = kinds.iter().any(|k| {
            k.starts_with("an engine event handler") || k.starts_with("a remote or custom")
        });
        let only = kinds.iter().all(|k| {
            k.starts_with("an engine event handler") || k.starts_with("a remote or custom")
        });
        let what = if kinds.contains("no writer found") {
            "no writer found: the value is the initial one in every run"
        } else if only {
            "written only by event handlers"
        } else if event {
            "written by event handlers and by functions or fragments"
        } else {
            "written only by functions, fragments or accessors"
        };
        *c.entry(format!("4 function 629 conditions of scene phases\t{what}"))
            .or_default() += readers.len() as u64;
        for kd in kinds {
            if kd.starts_with("an engine event handler") {
                *c.entry(format!(
                    "4 function 629 conditions of scene phases whose variable a handler writes\t{kd}"
                ))
                .or_default() += readers.len() as u64;
            }
        }
    }
    let mut out = String::from(
        "# scene_variable_census: kinds and counts only (spec/vm.md, Scenes offered again when a script variable is written). Sections: 1 phase conditions of every scene by function (629 reads a quest script variable; 660 carries a CIS2 text and is not evaluated);\n# 2 the function 629 conditions of scene phases; 3 the distinct (quest, variable) pairs they read and the instructions that write each; 4 those conditions by the kinds of writer of their variable.\n",
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
