#![forbid(unsafe_code)]
//! Array variables, array properties and the array opcodes of the compiled
//! scripts, and the array values the records' script blocks store
//! (`spec/vm.md`, None arrays): `array_census MANIFEST OUT.txt NAMES.txt`.
//!
//! OUT.txt holds counts and kinds only (`spec/array-census.txt`); NAMES.txt
//! names the scripts and properties and is never versioned. Nothing runs
//! but the binder: every script entry of every winning VMAD that stores a
//! value for an array property is instantiated in a fresh interpreter with
//! no host and given its values as binding gives them, and the variable
//! behind the property is read back.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{
    typed::Store,
    vmad::{Property, PropertyValue, Script as VScript, Vmad},
};
use fo4_vfs::Vfs;
use fo4_vm::{bind, setters, Lit, MapLibrary, NoHost, Script, Val, Vm};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    rc::Rc,
};

type E = Box<dyn std::error::Error>;
type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}

/// The array opcodes and the slot of the array they work on
/// (`pex_array_ops_v1`).
const ARRAY_OPS: [(u8, usize); 9] = [
    (31, 1),
    (32, 1),
    (33, 0),
    (34, 0),
    (40, 0),
    (42, 0),
    (43, 0),
    (45, 0),
    (46, 0),
];

/// Destination slot under the candidate meanings (as `vm_checks`).
fn dest(op: u8) -> Option<usize> {
    match op {
        1..=19 | 27 | 30 | 31 | 32 | 36 | 37 | 38 => Some(0),
        23 | 25 | 28 => Some(2),
        24 | 34 | 40 => Some(1),
        _ => None,
    }
}

fn family(elem: &str) -> &'static str {
    match elem {
        "int" | "float" | "bool" | "string" => "a simple type",
        "var" => "var",
        e if e.contains('#') => "a struct",
        _ => "an object type",
    }
}

fn chain(lib: &MapLibrary, name: &str) -> Vec<Rc<Script>> {
    let mut out = Vec::new();
    let mut t = name.to_string();
    while let Some(s) = lib.scripts.get(&t) {
        t = s.parent.clone();
        out.push(s.clone());
        if t.is_empty() || out.len() >= fo4_vm::MAX_PARENTS {
            break;
        }
    }
    out
}

/// Where an identifier of a function resolves.
enum Place {
    Param,
    Local,
    /// (declaring script, variable).
    Var(String, String),
    SelfRef,
    Other,
}

fn place(lib: &MapLibrary, f: &fo4_vm::Func, n: &str) -> Place {
    if f.params.iter().any(|p| p.0 == n) {
        return Place::Param;
    }
    if f.locals.iter().any(|p| p.0 == n) {
        return Place::Local;
    }
    if n == "self" {
        return Place::SelfRef;
    }
    for s in chain(lib, &f.script) {
        if s.variables.iter().any(|v| v.name == n) {
            return Place::Var(s.name.clone(), n.to_string());
        }
    }
    Place::Other
}

fn decl_type(lib: &MapLibrary, f: &fo4_vm::Func, n: &str) -> Option<String> {
    if n == "self" {
        return Some(f.script.clone());
    }
    if let Some(p) = f.params.iter().chain(&f.locals).find(|p| p.0 == n) {
        return Some(p.1.clone());
    }
    chain(lib, &f.script)
        .iter()
        .find_map(|s| s.variable(n).map(|v| v.type_name.clone()))
}

fn functions(s: &Script) -> Vec<Rc<fo4_vm::Func>> {
    let mut v: Vec<_> = s.functions.values().cloned().collect();
    for p in &s.properties {
        v.extend(p.read.iter().cloned());
        v.extend(p.write.iter().cloned());
    }
    v
}

/// Every (declaring script, variable) some instruction stores into: the
/// destination of an opcode under its candidate meaning, or a property
/// write (29) on a receiver whose declared type's chain declares the
/// property with a variable.
fn written(lib: &MapLibrary) -> BTreeSet<(String, String)> {
    let mut w = BTreeSet::new();
    for s in lib.scripts.values() {
        for f in functions(s) {
            for ins in &f.code {
                if let Some(d) = dest(ins.op) {
                    if let Some(Lit::Ident(n)) = ins.args.get(d) {
                        if let Place::Var(sc, v) = place(lib, &f, n) {
                            w.insert((sc, v));
                        }
                    }
                }
                if ins.op == 29 {
                    let (Some(Lit::Ident(p)), Some(Lit::Ident(r))) =
                        (ins.args.first(), ins.args.get(1))
                    else {
                        continue;
                    };
                    let Some(ty) = decl_type(lib, &f, r) else {
                        continue;
                    };
                    for sc in chain(lib, &ty) {
                        if let Some(pd) = sc.property(p) {
                            if let Some(v) = &pd.variable {
                                w.insert((sc.name.clone(), v.clone()));
                            }
                            break;
                        }
                    }
                }
            }
        }
    }
    w
}

/// What stored the temporary an array opcode reads: the nearest earlier
/// instruction in stored order whose destination it is.
fn temp_source(f: &fo4_vm::Func, at: usize, n: &str) -> &'static str {
    for ins in f.code[..at].iter().rev() {
        let Some(d) = dest(ins.op) else { continue };
        if ins.args.get(d) == Some(&Lit::Ident(n.to_string())) {
            return match ins.op {
                28 => "a temporary from a property read (28)",
                23..=25 => "a temporary from a call's result",
                38 => "a temporary from a struct member (38)",
                13 | 14 => "a temporary from a copy or cast (13, 14)",
                30 => "a temporary from a new array (30)",
                _ => "a temporary from another opcode",
            };
        }
    }
    "a temporary no earlier instruction stores"
}

/// Section 1 and 2: the compiled scripts.
fn scripts(lib: &MapLibrary, w: &BTreeSet<(String, String)>, c: &mut Counts) {
    *c.entry("1 scripts".into()).or_default() += lib.scripts.len() as u64;
    for s in lib.scripts.values() {
        for v in &s.variables {
            let Some(elem) = v.type_name.strip_suffix("[]") else {
                continue;
            };
            bump(c, format!("1 array variables by element\t{}", family(elem)));
            if v.init != Lit::None {
                bump(
                    c,
                    "1 array variables with a compiled initial value other than none",
                );
            }
            let behind = s
                .properties
                .iter()
                .any(|p| p.variable.as_deref() == Some(v.name.as_str()));
            let wr = w.contains(&(s.name.clone(), v.name.clone()));
            bump(
                c,
                format!(
                    "1 array variables\t{}\t{}",
                    if behind {
                        "behind a property"
                    } else {
                        "behind no property"
                    },
                    if wr {
                        "an instruction stores into it"
                    } else {
                        "no instruction stores into it"
                    }
                ),
            );
        }
        for p in &s.properties {
            let Some(elem) = p.type_name.strip_suffix("[]") else {
                continue;
            };
            bump(
                c,
                format!(
                    "1 array properties\t{}\telement {}",
                    if p.variable.is_some() {
                        "with a variable"
                    } else {
                        "with functions only"
                    },
                    family(elem)
                ),
            );
        }
        for f in functions(s) {
            for (i, ins) in f.code.iter().enumerate() {
                let Some(&(op, slot)) = ARRAY_OPS.iter().find(|x| x.0 == ins.op) else {
                    continue;
                };
                bump(c, format!("2 array opcode sites by opcode\t{op}"));
                let class = match ins.args.get(slot) {
                    Some(Lit::None) => "the none literal".to_string(),
                    Some(Lit::Ident(n)) => match place(lib, &f, n) {
                        Place::Param => "a parameter".into(),
                        Place::Local if n.starts_with("::temp") => temp_source(&f, i, n).into(),
                        Place::Local => "a local".into(),
                        Place::Var(sc, v) => {
                            let behind = lib.scripts.get(&sc).is_some_and(|d| {
                                d.properties
                                    .iter()
                                    .any(|p| p.variable.as_deref() == Some(v.as_str()))
                            });
                            format!(
                                "a script variable {}, {}",
                                if behind {
                                    "behind a property"
                                } else {
                                    "behind no property"
                                },
                                if w.contains(&(sc, v)) {
                                    "an instruction stores into it"
                                } else {
                                    "no instruction stores into it"
                                }
                            )
                        }
                        Place::SelfRef => "self".into(),
                        Place::Other => "another name".into(),
                    },
                    _ => "another literal".into(),
                };
                bump(c, format!("2 array opcode sites by operand\t{class}"));
            }
        }
    }
}

/// One script entry of a VMAD, with what it was read from.
struct Entry<'a> {
    part: &'static str,
    script: &'a VScript<'a>,
}

fn entries<'a>(v: &'a Vmad<'a>) -> Vec<Entry<'a>> {
    let mut out = Vec::new();
    for s in &v.scripts {
        out.push(Entry {
            part: "own script",
            script: s,
        });
    }
    if let Some(t) = &v.terminal {
        out.push(Entry {
            part: "fragment block",
            script: &t.script,
        });
    }
    if let Some(q) = &v.quest {
        out.push(Entry {
            part: "fragment block",
            script: &q.script,
        });
        for a in &q.aliases {
            for s in &a.scripts {
                out.push(Entry {
                    part: "alias script",
                    script: s,
                });
            }
        }
    }
    for b in [v.info.as_ref(), v.package.as_ref()].into_iter().flatten() {
        out.push(Entry {
            part: "fragment block",
            script: &b.script,
        });
    }
    if let Some(s) = &v.scene {
        out.push(Entry {
            part: "fragment block",
            script: &s.block.script,
        });
    }
    if let Some(p) = &v.perk {
        out.push(Entry {
            part: "fragment block",
            script: &p.script,
        });
    }
    out.retain(|e| !e.script.name.is_empty());
    out
}

fn stored_kind(v: &PropertyValue<'_>) -> String {
    match v {
        PropertyValue::Array(k, items) => format!(
            "an array of kind {}, {}",
            u16::from(*k) + 10,
            if items.is_empty() {
                "empty"
            } else {
                "one element or more"
            }
        ),
        PropertyValue::StructArray(items) => format!(
            "an array of structs (kind 17), {}",
            if items.is_empty() {
                "empty"
            } else {
                "one element or more"
            }
        ),
        PropertyValue::None => "kind 0 (none)".into(),
        _ => "a value that is not an array".into(),
    }
}

fn val_kind(v: &Val) -> &'static str {
    match v {
        Val::None => "none",
        Val::Unknown(_) => "an undetermined value",
        _ => "a value that is not an array",
    }
}

fn stored_len(v: &PropertyValue<'_>) -> Option<usize> {
    match v {
        PropertyValue::Array(_, items) => Some(items.len()),
        PropertyValue::StructArray(items) => Some(items.len()),
        _ => None,
    }
}

/// Section 3: what the records store for array properties and what the
/// binder makes of it.
#[allow(clippy::too_many_arguments)]
fn record_values(
    lib: &mut MapLibrary,
    store: &Store,
    w: &BTreeSet<(String, String)>,
    c: &mut Counts,
    left: &mut BTreeMap<String, (u64, bool)>,
    names: &mut String,
    b: &Budget,
) -> Result<(), E> {
    let winners: Vec<_> = store
        .winners()
        .filter(|x| !x.deleted)
        .map(|x| store.overrides().form_key(x.key, store.order()))
        .collect();
    for k in &winners {
        let bb = b.child();
        let Some((r, plugin)) = store.raw_with_plugin(k, &bb)? else {
            continue;
        };
        let Some(sub) = r.subrecords.iter().find(|x| x.signature == *b"VMAD") else {
            continue;
        };
        let file = r.source.file.clone();
        let Ok(v) = Vmad::read(r.data(sub), r.signature, &file, sub.offset, &bb) else {
            bump(c, "3 VMAD that does not read");
            continue;
        };
        for e in entries(&v) {
            let name = fo4_pex::index::lower(e.script.name);
            let ch = chain(lib, &name);
            if ch.is_empty() {
                bump(c, "3 script entries whose script no archive holds");
                continue;
            }
            bump(c, format!("3 script entries\t{}", e.part));
            // Declared array properties, nearest declaration first.
            let mut declared: BTreeMap<String, (String, Option<String>)> = BTreeMap::new();
            for s in ch.iter().rev() {
                for p in &s.properties {
                    if p.type_name.ends_with("[]") {
                        declared.insert(p.name.clone(), (s.name.clone(), p.variable.clone()));
                    } else {
                        declared.remove(&p.name);
                    }
                }
            }
            let stored: BTreeMap<String, &Property<'_>> = e
                .script
                .properties
                .iter()
                .map(|p| (fo4_pex::index::lower(p.name), p))
                .collect();
            for (n, p) in &stored {
                if stored_len(&p.value).is_some() && !declared.contains_key(n) {
                    bump(
                        c,
                        "3 array values stored for a property not declared as an array",
                    );
                }
            }
            let mut check = Vec::new();
            for (n, (sc, var)) in &declared {
                let key = format!("{sc}.{n}");
                match stored.get(n) {
                    None => {
                        let wr = var
                            .as_ref()
                            .is_some_and(|v| w.contains(&(sc.clone(), v.clone())));
                        bump(
                            c,
                            format!(
                                "3 array properties of a script entry\tnot stored by the record\t{}\t{}",
                                e.part,
                                if var.is_none() {
                                    "a property with functions only"
                                } else if wr {
                                    "an instruction stores into its variable"
                                } else {
                                    "no instruction stores into its variable: none until a script sets it"
                                }
                            ),
                        );
                        let slot = left.entry(key).or_insert((0, wr));
                        slot.0 += 1;
                    }
                    Some(p) => {
                        bump(
                            c,
                            format!(
                                "3 array properties of a script entry\tstored by the record\t{}\t{}",
                                e.part,
                                stored_kind(&p.value)
                            ),
                        );
                        if let (Some(len), Some(var)) = (stored_len(&p.value), var) {
                            check.push((n.clone(), var.clone(), len, key));
                        }
                    }
                }
            }
            if check.is_empty() {
                continue;
            }
            *c.entry("3 binder: stored array values checked".into())
                .or_default() += check.len() as u64;
            // The binder, as binding runs it (`vmad_property_sets_variable_v1`).
            let mut host = NoHost;
            let mut vm = Vm::new(lib, &mut host);
            let inst = match vm.instantiate(None, &name) {
                Ok(i) => i,
                Err(_) => {
                    for _ in &check {
                        bump(
                            c,
                            "3 binder: stored array values\tthe script does not instantiate",
                        );
                    }
                    continue;
                }
            };
            let mut notes = Vec::new();
            if bind::apply_script(
                &mut vm,
                store,
                plugin,
                &file,
                inst,
                &name,
                &e.script.properties,
                &mut notes,
                &bb,
            )
            .is_err()
            {
                for _ in &check {
                    bump(
                        c,
                        "3 binder: stored array values\tthe binder stopped with an error",
                    );
                }
                continue;
            }
            for (n, var, len, key) in check {
                let slot = vm.world.instances[inst]
                    .vars
                    .iter()
                    .find(|s| s.name == var)
                    .map(|s| s.value.clone());
                let how = match slot {
                    Some(Val::Array(a)) if vm.world.arrays[a].items.len() == len => {
                        let items = &vm.world.arrays[a].items;
                        let unknown = items
                            .iter()
                            .filter(|x| matches!(x, Val::Unknown(_)))
                            .count();
                        let none = items.iter().filter(|x| matches!(x, Val::None)).count();
                        *c.entry("3 binder: elements carried".into()).or_default() += len as u64;
                        *c.entry("3 binder: elements carried as undetermined (an object value naming no winning record)".into())
                            .or_default() += unknown as u64;
                        *c.entry("3 binder: elements carried as none (FormID 0 or a struct the script does not declare)".into())
                            .or_default() += none as u64;
                        "carried: an array of the stored length".to_string()
                    }
                    Some(Val::Array(_)) => "not carried: an array of another length".into(),
                    Some(other) => {
                        let why = notes
                            .iter()
                            .find(|x| x.contains(&format!("property {n}")))
                            .cloned()
                            .unwrap_or_default();
                        names.push_str(&format!("binder did not carry {key}: {why}\n"));
                        format!("not carried: the variable holds {}", val_kind(&other))
                    }
                    None => "not carried: no variable of that name".into(),
                };
                bump(c, format!("3 binder: stored array values\t{how}"));
            }
        }
    }
    Ok(())
}

fn run() -> Result<(), E> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 3 {
        return Err("array_census MANIFEST OUT.txt NAMES.txt".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let b = Budget::new(m.limits.max_memory);
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let mut lib = setters::read_all_scripts(&vfs, m.limits.max_memory)?;
    let w = written(&lib);
    let mut c = Counts::new();
    scripts(&lib, &w, &mut c);
    let mut left: BTreeMap<String, (u64, bool)> = BTreeMap::new();
    let mut names = String::new();
    record_values(&mut lib, &store, &w, &mut c, &mut left, &mut names, &b)?;
    // Counts that are zero in the measured data, listed so the census says so.
    for k in [
        "1 array variables with a compiled initial value other than none",
        "3 array values stored for a property not declared as an array",
        "3 VMAD that does not read",
    ] {
        c.entry(k.into()).or_default();
    }
    for (k, (n, wr)) in &left {
        bump(
            &mut c,
            format!(
                "4 declared array properties left none by at least one script entry\t{}",
                if *wr {
                    "an instruction stores into the variable"
                } else {
                    "no instruction stores into the variable"
                }
            ),
        );
        names.push_str(&format!(
            "left none\t{k}\t{n} script entries\t{}\n",
            if *wr {
                "an instruction stores into it"
            } else {
                "no instruction stores into it"
            }
        ));
    }
    let mut out = String::from(
        "# array_census: counts and kinds only (spec/vm.md, None arrays)\n\
         # 1 array variables and properties of the compiled scripts; 2 the array opcodes by the operand they work on;\n\
         # 3 what the records' script entries store for array properties and what the binder carries; 4 properties left none\n",
    );
    for (k, v) in &c {
        out.push_str(&format!("{v}\t{k}\n"));
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
