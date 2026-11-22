#![forbid(unsafe_code)]
//! Checks that could refute the candidate meanings of the opcodes without
//! the running game (`spec/vm.md`, Checks): slot types under each meaning,
//! data flow (temporaries written before they are read, destinations read
//! later), compiler idioms that must hold if a meaning is right, and the
//! execution of the script functions that reach no native function under
//! the candidate meanings and under alternative readings
//! (`fo4_vm::Variant`).
//!
//! `vm_checks MANIFEST OUT_DIR` writes `vm-checks.txt` (counts only, the
//! file committed as `spec/vm-checks.txt`) and `vm-checks-names.txt`
//! (examples by name; never versioned).
use fo4_core::{Budget, Manifest};
use fo4_pex::Pex;
use fo4_vfs::Vfs;
use fo4_vm::{
    program::{Func, Ins, Lit, MapLibrary, Script},
    NoHost, Outcome, Val, Variant, Vm,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::Write as _,
    path::Path,
    rc::Rc,
};

type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}
fn add(m: &mut Counts, k: impl Into<String>, n: u64) {
    *m.entry(k.into()).or_default() += n;
}

/// Lower-case declared types of the names visible in a function.
fn env(lib: &MapLibrary, s: &Script, f: &Func) -> HashMap<String, String> {
    let mut chain = Vec::new();
    let mut t = s.name.clone();
    while let Some(x) = lib.scripts.get(&t) {
        chain.push(x.clone());
        if x.parent.is_empty() || chain.len() > 64 {
            break;
        }
        t = x.parent.clone();
    }
    let mut m = HashMap::new();
    for x in chain.iter().rev() {
        for v in &x.variables {
            m.insert(v.name.clone(), v.type_name.clone());
        }
    }
    m.insert("self".into(), s.name.clone());
    m.insert("::state".into(), "string".into());
    for (n, t) in f.params.iter().chain(&f.locals) {
        m.insert(n.clone(), t.clone());
    }
    m
}

fn lit_type(env: &HashMap<String, String>, l: &Lit) -> Option<String> {
    Some(match l {
        Lit::Ident(n) => env.get(n)?.clone(),
        Lit::None => "none".into(),
        Lit::Str(_) => "string".into(),
        Lit::Int(_) => "int".into(),
        Lit::Float(_) => "float".into(),
        Lit::Bool(_) => "bool".into(),
    })
}

fn chain_of<'l>(lib: &'l MapLibrary, name: &str) -> Vec<&'l Rc<Script>> {
    let mut out = Vec::new();
    let mut t = name.to_string();
    while let Some(x) = lib.scripts.get(&t) {
        out.push(x);
        if x.parent.is_empty() || out.len() > 64 {
            break;
        }
        t = x.parent.clone();
    }
    out
}

fn property_type(lib: &MapLibrary, ty: &str, name: &str) -> Option<String> {
    chain_of(lib, ty)
        .into_iter()
        .find_map(|s| s.property(name).map(|p| p.type_name.clone()))
}

fn member_type(lib: &MapLibrary, ty: &str, member: &str) -> Option<String> {
    let (s, st) = ty.split_once('#')?;
    let d = lib.scripts.get(s)?.structs.iter().find(|d| d.name == st)?;
    d.members
        .iter()
        .find(|m| m.0 == member)
        .map(|m| m.1.clone())
}

fn elem(t: &str) -> Option<&str> {
    t.strip_suffix("[]")
}

/// The candidate result-type relation of an opcode, when it has one:
/// `Some(true)` when the slots' declared types satisfy it.
fn type_check(
    lib: &MapLibrary,
    env: &HashMap<String, String>,
    ins: &Ins,
) -> Option<(String, bool)> {
    let a = &ins.args;
    let t = |i: usize| a.get(i).and_then(|l| lit_type(env, l));
    let id = |i: usize| match a.get(i) {
        Some(Lit::Ident(n)) => Some(n.clone()),
        _ => None,
    };
    let (what, ok) = match ins.op {
        1 | 3 | 5 | 7 | 9 | 11 => ("slot 0 is Int", t(0)? == "int"),
        2 | 4 | 6 | 8 | 12 => ("slot 0 is Float", t(0)? == "float"),
        10 | 15..=19 | 36 => ("slot 0 is Bool", t(0)? == "bool"),
        27 => ("slot 0 is String", t(0)? == "string"),
        30 => ("slot 0 is an array", t(0)?.ends_with("[]")),
        31 => (
            "slot 0 is Int and slot 1 an array",
            t(0)? == "int" && t(1)?.ends_with("[]"),
        ),
        32 => (
            "slot 0 has the element type of slot 1",
            Some(t(0)?.as_str()) == elem(&t(1)?),
        ),
        33 => ("slot 2 has the element type of slot 0", {
            let v = t(2)?;
            Some(v.as_str()) == elem(&t(0)?) || v == "none"
        }),
        34 => ("slot 1 is Int, slot 2 has the element type of slot 0", {
            let v = t(2)?;
            t(1)? == "int" && (Some(v.as_str()) == elem(&t(0)?) || v == "none")
        }),
        40 => (
            "slot 1 is Int, slot 0 an array of structs",
            t(1)? == "int" && t(0)?.contains('#'),
        ),
        42 | 43 => ("slot 1 has the element type of slot 0", {
            let v = t(1)?;
            Some(v.as_str()) == elem(&t(0)?) || v == "none"
        }),
        37 => ("slot 0 is a struct", t(0)?.contains('#')),
        38 => (
            "slot 0 has the member's type",
            Some(t(0)?) == member_type(lib, &t(1)?, &id(2)?),
        ),
        39 => ("slot 2 has the member's type", {
            let v = t(2)?;
            Some(v.clone()) == member_type(lib, &t(0)?, &id(1)?) || v == "none"
        }),
        13 => (
            "slots 0 and 1 have one type (none into object, array or struct slots)",
            {
                let (x, y) = (t(0)?, t(1)?);
                x == y
                    || (y == "none" && !matches!(x.as_str(), "int" | "float" | "bool" | "string"))
            },
        ),
        14 => ("slots 0 and 1 have different types", t(0)? != t(1)?),
        28 | 29 => (
            "slot 2 has the property's type",
            Some(t(2)?) == property_type(lib, &t(1)?, &id(0)?) || t(2)? == "none",
        ),
        _ => return None,
    };
    Some((what.to_string(), ok))
}

/// Destination slot under the candidate meanings.
fn dest(op: u8) -> Option<usize> {
    match op {
        1..=19 | 27 | 30 | 31 | 32 | 36 | 37 | 38 => Some(0),
        23 | 25 | 28 => Some(2),
        24 | 34 | 40 => Some(1),
        _ => None,
    }
}

/// Successors of an instruction under the candidate jump meanings; the
/// instruction count stands for the function's end.
fn succ(code: &[Ins], i: usize) -> Vec<usize> {
    let n = code.len() as i64;
    let rel = |d: &Lit| match d {
        Lit::Int(d) => Some((i as i64 + i64::from(*d)).clamp(0, n) as usize),
        _ => None,
    };
    match code[i].op {
        20 => rel(&code[i].args[0]).into_iter().collect(),
        21 | 22 => {
            let mut v = vec![i + 1];
            v.extend(rel(&code[i].args[1]));
            v
        }
        26 => vec![code.len()],
        _ => vec![i + 1],
    }
}

fn is_temp(n: &str) -> bool {
    n.starts_with("::temp")
}

/// Identifiers an instruction reads when its destination is `d`.
fn reads(ins: &Ins, d: Option<usize>) -> Vec<&str> {
    let names: Vec<&str> = match ins.op {
        // Name slots of calls and properties are not values.
        23 => ins
            .args
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 0)
            .map(|x| x.1)
            .chain(&ins.extra)
            .filter_map(name)
            .collect(),
        24 => ins
            .args
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 0)
            .map(|x| x.1)
            .chain(&ins.extra)
            .filter_map(name)
            .collect(),
        25 => ins
            .args
            .iter()
            .enumerate()
            .filter(|(i, _)| *i > 1)
            .map(|x| x.1)
            .chain(&ins.extra)
            .filter_map(name)
            .collect(),
        28 | 29 => ins
            .args
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 0)
            .filter_map(|x| name(x.1))
            .collect(),
        36 => ins.args.iter().take(2).filter_map(name).collect(),
        38 => ins.args.iter().take(2).filter_map(name).collect(),
        39 => [&ins.args[0], &ins.args[2]]
            .into_iter()
            .filter_map(name)
            .collect(),
        _ => ins.args.iter().filter_map(name).collect(),
    };
    let dn = d.and_then(|k| ins.args.get(k)).and_then(name);
    let mut out = Vec::new();
    let mut dropped = false;
    for n in names {
        if Some(n) == dn && !dropped {
            dropped = true;
            continue;
        }
        out.push(n);
    }
    out
}
fn name(l: &Lit) -> Option<&str> {
    match l {
        Lit::Ident(n) => Some(n),
        _ => None,
    }
}

/// Reads of temporaries not written on every path before them, with the
/// destination of each opcode given by `d`.
fn undefined_reads(f: &Func, d: &dyn Fn(u8) -> Option<usize>) -> u64 {
    let temps: Vec<&str> = f
        .locals
        .iter()
        .map(|x| x.0.as_str())
        .filter(|n| is_temp(n))
        .collect();
    if temps.is_empty() || f.code.is_empty() {
        return 0;
    }
    let ix = |n: &str| temps.iter().position(|t| *t == n);
    let n = f.code.len();
    // Must-assigned sets at instruction entry; None = not reached yet.
    let mut at: Vec<Option<Vec<bool>>> = vec![None; n + 1];
    at[0] = Some(vec![false; temps.len()]);
    let mut work = vec![0usize];
    while let Some(i) = work.pop() {
        if i >= n {
            continue;
        }
        let Some(mut s) = at[i].clone() else { continue };
        if let Some(k) = d(f.code[i].op) {
            if let Some(t) = f.code[i].args.get(k).and_then(name).and_then(ix) {
                s[t] = true;
            }
        }
        for j in succ(&f.code, i) {
            let changed = match &mut at[j] {
                None => {
                    at[j] = Some(s.clone());
                    true
                }
                Some(old) => {
                    let mut c = false;
                    for (o, x) in old.iter_mut().zip(&s) {
                        if *o && !x {
                            *o = false;
                            c = true;
                        }
                    }
                    c
                }
            };
            if changed {
                work.push(j);
            }
        }
    }
    let mut bad = 0;
    for (i, ins) in f.code.iter().enumerate() {
        let Some(s) = &at[i] else { continue };
        for r in reads(ins, d(ins.op)) {
            if let Some(t) = ix(r) {
                if !s[t] {
                    bad += 1;
                }
            }
        }
    }
    bad
}

/// Writes of temporaries (candidate destinations) that no later read uses.
fn dead_writes(f: &Func, counts: &mut Counts) {
    let n = f.code.len();
    let temps: Vec<&str> = f
        .locals
        .iter()
        .map(|x| x.0.as_str())
        .filter(|n| is_temp(n))
        .collect();
    if temps.is_empty() {
        return;
    }
    let ix = |n: &str| temps.iter().position(|t| *t == n);
    let mut live: Vec<Vec<bool>> = vec![vec![false; temps.len()]; n + 1];
    let mut changed = true;
    while changed {
        changed = false;
        for i in (0..n).rev() {
            let mut out = vec![false; temps.len()];
            for j in succ(&f.code, i) {
                for (o, x) in out.iter_mut().zip(&live[j]) {
                    *o |= *x;
                }
            }
            if let Some(k) = dest(f.code[i].op) {
                if let Some(t) = f.code[i].args.get(k).and_then(name).and_then(ix) {
                    out[t] = false;
                }
            }
            for r in reads(&f.code[i], dest(f.code[i].op)) {
                if let Some(t) = ix(r) {
                    out[t] = true;
                }
            }
            if out != live[i] {
                live[i] = out;
                changed = true;
            }
        }
    }
    for (i, ins) in f.code.iter().enumerate() {
        let Some(k) = dest(ins.op) else { continue };
        let Some(t) = ins.args.get(k).and_then(name).and_then(ix) else {
            continue;
        };
        let used = succ(&f.code, i).iter().any(|j| live[*j][t]);
        bump(
            counts,
            format!("op {:02} temporary destination read later: {used}", ins.op),
        );
    }
}

/// What a conditional jump's operand says about an object identifier
/// under the candidate meanings: (object, operand is true exactly when the
/// object is not none, how). Copies by opcodes 13 and 14 between Bool
/// temporaries are followed back while no instruction on the way is a
/// jump target.
fn guard_of(
    f: &Func,
    env: &HashMap<String, String>,
    targets: &BTreeSet<usize>,
    i: usize,
) -> Option<(String, bool, &'static str)> {
    let is_obj = |n: &str| {
        env.get(n).is_some_and(|t| {
            !matches!(
                t.as_str(),
                "int" | "float" | "bool" | "string" | "var" | "none"
            ) && !t.ends_with("[]")
                && !t.contains('#')
        })
    };
    let mut c = name(&f.code[i].args[0])?.to_string();
    if is_obj(&c) {
        return Some((c, true, "the object itself"));
    }
    if targets.contains(&i) {
        return None;
    }
    let mut j = i;
    loop {
        j = j.checked_sub(1)?;
        let p = &f.code[j];
        if name(p.args.first()?)? != c {
            return None;
        }
        let from = p.args.get(1).and_then(name);
        match p.op {
            13 | 14 if from.is_some_and(|x| env.get(x).is_some_and(|t| t == "bool")) => {
                if targets.contains(&j) {
                    return None;
                }
                c = from?.to_string();
                continue;
            }
            14 => {
                let x = from?;
                return is_obj(x).then(|| (x.to_string(), true, "a cast of the object to Bool"));
            }
            10 => {
                let x = from?;
                return is_obj(x).then(|| (x.to_string(), false, "opcode 10 of the object"));
            }
            15 => {
                let x = match (p.args.get(1)?, p.args.get(2)?) {
                    (Lit::Ident(x), Lit::None) | (Lit::None, Lit::Ident(x)) => x,
                    _ => return None,
                };
                return is_obj(x).then(|| (x.clone(), false, "opcode 15 of the object and none"));
            }
            _ => return None,
        }
    }
}

/// Whether instructions from `start` up to the next jump or jump target
/// call a method or property on `x` before writing it.
fn uses_as_receiver(f: &Func, targets: &BTreeSet<usize>, start: usize, x: &str) -> bool {
    for (j, ins) in f.code.iter().enumerate().skip(start) {
        if j > start && targets.contains(&j) {
            return false;
        }
        match ins.op {
            23 | 28 | 29 if name(&ins.args[1]) == Some(x) => return true,
            20..=22 | 26 => return false,
            _ => {}
        }
        if dest(ins.op).and_then(|k| ins.args.get(k)).and_then(name) == Some(x) {
            return false;
        }
    }
    false
}

/// Sample values of a simple or array type for executing functions.
fn samples(vm: &mut Vm<'_>, t: &str) -> Option<Vec<Val>> {
    let s = |x: &str| Val::Str(x.into());
    Some(match t {
        "int" => [0, 1, 2, 5, -1, 10].map(Val::Int).to_vec(),
        "float" => [0.0, 0.5, 1.0, -2.5, 10.0].map(Val::Float).to_vec(),
        "bool" => vec![Val::Bool(false), Val::Bool(true)],
        "string" => vec![s(""), s("a"), s("Abc")],
        _ => {
            let e = t.strip_suffix("[]")?;
            let lists: Vec<Vec<Val>> = match e {
                "int" => vec![
                    vec![],
                    vec![Val::Int(1), Val::Int(2), Val::Int(3)],
                    [5, -1, 5, 0].map(Val::Int).to_vec(),
                ],
                "float" => vec![vec![], vec![Val::Float(0.5), Val::Float(2.0)]],
                "bool" => vec![vec![], vec![Val::Bool(true), Val::Bool(false)]],
                "string" => vec![vec![], vec![s("a"), s("b")]],
                _ => return None,
            };
            lists.into_iter().map(|l| vm.world.array(e, l)).collect()
        }
    })
}

/// Statically pure: scripted; every call on self resolves, in every state
/// of the instance's script chain, to scripted pure functions, and every
/// static or parent call to one; no call or property on another object;
/// no variable or self property of a type other than Int, Float, Bool or
/// String (an instance made without VMAD holds none there).
fn pure(
    lib: &MapLibrary,
    inst: Option<&str>,
    f: &Func,
    memo: &mut HashMap<(String, String, String), bool>,
    depth: usize,
) -> bool {
    let key = (
        inst.unwrap_or("").to_string(),
        f.script.clone(),
        format!("{}.{}", f.state, f.name),
    );
    if let Some(v) = memo.get(&key) {
        return *v;
    }
    if depth > 32 || f.native() {
        return false;
    }
    memo.insert(key.clone(), true);
    let simple = |t: &str| matches!(t, "int" | "float" | "bool" | "string");
    let all_named = |ty: &str, n: &str| -> Vec<Rc<Func>> {
        chain_of(lib, ty)
            .into_iter()
            .flat_map(|x| {
                x.functions
                    .iter()
                    .filter(|(k, _)| k.1 == n)
                    .map(|x| x.1.clone())
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    let first = |ty: &str, n: &str| -> Vec<Rc<Func>> {
        for x in chain_of(lib, ty) {
            if let Some(g) = x.function("", n) {
                return vec![g.clone()];
            }
        }
        Vec::new()
    };
    let Some(owner) = lib.scripts.get(&f.script) else {
        return false;
    };
    let e = env(lib, owner, f);
    let locals: BTreeSet<&str> = f
        .params
        .iter()
        .chain(&f.locals)
        .map(|x| x.0.as_str())
        .collect();
    let mut ok = true;
    'ins: for ins in &f.code {
        for a in ins.args.iter().chain(&ins.extra) {
            if let Lit::Ident(n) = a {
                if !locals.contains(n.as_str()) && n != "self" && n != "::state" {
                    if let Some(t) = e.get(n) {
                        if !simple(t) {
                            ok = false;
                            break 'ins;
                        }
                    }
                }
            }
        }
        let (targets, self_inst) = match ins.op {
            23 => {
                if name(&ins.args[1]) != Some("self") {
                    ok = false;
                    break;
                }
                let Some(i) = inst else {
                    ok = false;
                    break;
                };
                (
                    name(&ins.args[0])
                        .map(|n| all_named(i, n))
                        .unwrap_or_default(),
                    inst,
                )
            }
            24 => {
                let Some(i) = inst else {
                    ok = false;
                    break;
                };
                let parent = owner.parent.clone();
                (
                    name(&ins.args[0])
                        .map(|n| all_named(&parent, n))
                        .unwrap_or_default(),
                    Some(i),
                )
            }
            25 => match (name(&ins.args[0]), name(&ins.args[1])) {
                (Some(t), Some(n)) => (first(t, n), None),
                _ => (Vec::new(), None),
            },
            28 | 29 => {
                let pt =
                    name(&ins.args[0]).and_then(|n| inst.and_then(|i| property_type(lib, i, n)));
                if name(&ins.args[1]) != Some("self") || !pt.as_deref().is_some_and(simple) {
                    ok = false;
                    break;
                }
                // A property with functions runs them.
                let fns: Vec<Rc<Func>> = name(&ins.args[0])
                    .and_then(|n| {
                        chain_of(lib, inst?)
                            .into_iter()
                            .find_map(|x| x.property(n).cloned())
                    })
                    .map(|p| p.read.iter().chain(&p.write).cloned().collect())
                    .unwrap_or_default();
                (fns, inst)
            }
            36 => {
                ok = false;
                break;
            }
            _ => continue,
        };
        if targets.is_empty() && !matches!(ins.op, 28 | 29) {
            ok = false;
            break;
        }
        for g in targets {
            if !pure(lib, self_inst, &g, memo, depth + 1) {
                ok = false;
                break 'ins;
            }
        }
    }
    memo.insert(key, ok);
    ok
}

fn outcome_class(o: &Outcome) -> String {
    match o {
        Outcome::Completed(Val::Unknown(_)) => "completed with an open result".into(),
        Outcome::Completed(_) => "completed".into(),
        Outcome::Blocked(b) => format!(
            "blocked ({})",
            match &b.on {
                fo4_vm::Origin::Rule(r, _) => format!("rule {r}"),
                fo4_vm::Origin::Native(_) => "native".into(),
                fo4_vm::Origin::NoInstance(..) => "no instance".into(),
            }
        ),
        Outcome::Failed(e) => format!("failed {}", e.kind()),
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("vm_checks MANIFEST OUT_DIR".into());
    }
    let started = std::time::Instant::now();
    let m = Manifest::load(Path::new(&args[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let c = &vfs.candidates;
    let mut lib = MapLibrary::default();
    for (i, x) in c.iter().enumerate() {
        let Some(k) = &x.key else { continue };
        if !k.ends_with(b".pex") || c.get(i + 1).and_then(|n| n.key.as_ref()) == Some(k) {
            continue;
        }
        let b = Budget::new(m.limits.max_memory);
        let bytes = vfs.read_candidate(x, &b)?;
        let path = String::from_utf8_lossy(k).into_owned();
        let p = Pex::read(&bytes, &path, &b)?;
        if let Some(s) = Script::from_pex(&p, 0) {
            lib.add(s);
        }
    }
    let mut counts = Counts::new();
    let mut types = BTreeMap::<String, [u64; 2]>::new();
    let mut names = String::new();
    let scripts: Vec<Rc<Script>> = lib.scripts.values().cloned().collect();
    add(&mut counts, "scripts", scripts.len() as u64);

    // Every function with its script.
    let mut funcs: Vec<(Rc<Script>, Rc<Func>)> = Vec::new();
    for s in &scripts {
        for f in s.functions.values() {
            funcs.push((s.clone(), f.clone()));
        }
        for p in &s.properties {
            for f in p.read.iter().chain(&p.write) {
                funcs.push((s.clone(), f.clone()));
            }
        }
    }
    add(&mut counts, "functions", funcs.len() as u64);

    // ---- A1 slot types -------------------------------------------------
    for (s, f) in &funcs {
        let e = env(&lib, s, f);
        for ins in &f.code {
            if let Some((what, ok)) = type_check(&lib, &e, ins) {
                types.entry(format!("op {:02} {what}", ins.op)).or_default()[usize::from(!ok)] += 1;
                if ins.op == 14 && !ok {
                    let t = lit_type(&e, &ins.args[0]).unwrap_or_default();
                    let t = match t.as_str() {
                        "bool" | "int" | "float" | "string" | "var" => t,
                        x if x.ends_with("[]") => "array".into(),
                        _ => "object".into(),
                    };
                    bump(
                        &mut counts,
                        format!("op 14 with one declared type in both slots: {t}"),
                    );
                }
            }
            if matches!(ins.op, 5..=8) {
                for (k, a) in ins.args.iter().enumerate().skip(1) {
                    let zero = matches!(a, Lit::Int(0)) || matches!(a, Lit::Float(x) if *x == 0.0);
                    if zero {
                        bump(
                            &mut counts,
                            format!("op {:02} with a literal zero in slot {k}", ins.op),
                        );
                    }
                }
            }
        }
    }

    // ---- A2 data flow ----------------------------------------------------
    let mut flow = Counts::new();
    let base: u64 = funcs.iter().map(|(_, f)| undefined_reads(f, &dest)).sum();
    add(
        &mut flow,
        "candidate destinations: temporary reads not written on every path before",
        base,
    );
    // Of those, reads of temporaries that no instruction of the function
    // writes at all (no reading of the opcodes can write them).
    let mut never = 0u64;
    for (_, f) in &funcs {
        if undefined_reads(f, &dest) == 0 {
            continue;
        }
        let written: BTreeSet<&str> = f
            .code
            .iter()
            .filter_map(|i| dest(i.op).and_then(|k| i.args.get(k)).and_then(name))
            .collect();
        for ins in &f.code {
            for r in reads(ins, dest(ins.op)) {
                if is_temp(r) && !written.contains(r) {
                    never += 1;
                }
            }
        }
    }
    add(&mut flow, "candidate destinations: of these, reads of temporaries no instruction of the function writes", never);
    for (s, f) in &funcs {
        if undefined_reads(f, &dest) > 0 {
            writeln!(
                names,
                "undefined temporary read under the candidate: {}.{}.{}",
                s.name, f.state, f.name
            )?;
        }
    }
    let ops: BTreeSet<u8> = funcs
        .iter()
        .flat_map(|(_, f)| f.code.iter().map(|i| i.op))
        .collect();
    for &op in &ops {
        if matches!(op, 20..=26) {
            continue;
        }
        let holding: Vec<&Rc<Func>> = funcs
            .iter()
            .filter(|(_, f)| f.code.iter().any(|i| i.op == op))
            .map(|x| &x.1)
            .collect();
        let slots: BTreeSet<usize> = holding
            .iter()
            .flat_map(|f| f.code.iter().filter(|i| i.op == op))
            .flat_map(|i| {
                i.args
                    .iter()
                    .enumerate()
                    .filter(|x| matches!(x.1, Lit::Ident(_)))
                    .map(|x| x.0)
            })
            .collect();
        let cand: u64 = holding.iter().map(|f| undefined_reads(f, &dest)).sum();
        let mut alts = vec![(None, "no destination".to_string())];
        for k in &slots {
            if Some(*k) != dest(op) {
                alts.push((Some(*k), format!("destination slot {k}")));
            }
        }
        if dest(op).is_none() {
            alts.retain(|a| a.0.is_some());
        }
        let mut line = format!(
            "op {op:02} in {} functions: candidate ({}) {cand}",
            holding.len(),
            dest(op).map_or("no destination".into(), |k| format!("destination slot {k}"))
        );
        for (k, label) in alts {
            let d = move |o: u8| if o == op { k } else { dest(o) };
            let n: u64 = holding.iter().map(|f| undefined_reads(f, &d)).sum();
            let _ = write!(line, "; {label} {n}");
        }
        add(&mut flow, line, 0);
    }
    let mut dead = Counts::new();
    for (_, f) in &funcs {
        dead_writes(f, &mut dead);
    }

    // ---- A3 idioms -------------------------------------------------------
    let mut idioms = Counts::new();
    for (s, f) in &funcs {
        let e = env(&lib, s, f);
        let code = &f.code;
        let mut targets = BTreeSet::new();
        for i in 0..code.len() {
            if matches!(code[i].op, 20..=22) {
                for t in succ(code, i) {
                    if t != i + 1 || code[i].op == 20 {
                        targets.insert(t);
                    }
                }
            }
        }
        for (i, ins) in code.iter().enumerate() {
            // Comparison results and what reads them first.
            if (15..=19).contains(&ins.op) {
                if let Some(t) = name(&ins.args[0]) {
                    let next = code.get(i + 1);
                    let k = match next {
                        Some(n) if matches!(n.op, 21 | 22) && name(&n.args[0]) == Some(t) => {
                            "read next by a conditional jump"
                        }
                        Some(n) if matches!(n.op, 13 | 14) && name(&n.args[1]) == Some(t) => {
                            "copied next"
                        }
                        Some(n) if n.op == 26 && name(&n.args[0]) == Some(t) => "returned next",
                        _ => "other",
                    };
                    bump(&mut idioms, format!("comparison result {k}"));
                }
            }
            // Conditional jump operand types.
            if matches!(ins.op, 21 | 22) {
                let t = match &ins.args[0] {
                    Lit::Ident(n) => {
                        e.get(n)
                            .map_or("undeclared".to_string(), |t| match t.as_str() {
                                "bool" | "int" | "float" | "string" | "var" => t.clone(),
                                x if x.ends_with("[]") => "array".into(),
                                x if x.contains('#') => "struct".into(),
                                _ => "object".into(),
                            })
                    }
                    Lit::Bool(b) => format!("literal {b}"),
                    other => format!("literal {other:?}"),
                };
                bump(&mut idioms, format!("op {} operand {t}", ins.op));
                // Literal operands: is it a loop head?
                if let Lit::Bool(b) = ins.args[0] {
                    let tgt = succ(code, i)
                        .into_iter()
                        .find(|x| *x != i + 1)
                        .unwrap_or(i + 1);
                    let back = code[..tgt.min(code.len())]
                        .iter()
                        .enumerate()
                        .skip(i + 1)
                        .any(|(j, x)| x.op == 20 && succ(code, j).first().is_some_and(|t| *t <= i));
                    bump(
                        &mut idioms,
                        format!("op {} on literal {b}: target past a backward jump to it or before: {back}", ins.op),
                    );
                    writeln!(
                        names,
                        "op {} on literal {b} in {}.{}.{} #{i}",
                        ins.op, s.name, f.state, f.name
                    )?;
                }
                // Object guards.
                if let Some((x, true_when_set, how)) = guard_of(f, &e, &targets, i) {
                    // Under the candidate, 21 jumps when its operand is
                    // true and 22 when it is false: the next instruction
                    // runs when the operand is false for 21, true for 22.
                    let operand_on_fallthrough = ins.op == 22;
                    let set_on_fallthrough = operand_on_fallthrough == true_when_set;
                    if uses_as_receiver(f, &targets, i + 1, &x) {
                        bump(
                            &mut idioms,
                            format!(
                                "op {} guard by {how} then a call or property on the object in the fall-through: object none there under the candidate: {}",
                                ins.op,
                                !set_on_fallthrough
                            ),
                        );
                        if !set_on_fallthrough {
                            writeln!(
                                names,
                                "call on an object that is none under the candidate: {}.{}.{} #{i}",
                                s.name, f.state, f.name
                            )?;
                        }
                    }
                }
            }
            // Loops guarding an array index.
            if ins.op == 20 {
                let Some(&k) = succ(code, i).first() else {
                    continue;
                };
                if k > i {
                    continue;
                }
                bump(&mut idioms, "backward jumps");
                // A conditional jump in the loop that exits just after it.
                let exit = (k..i)
                    .find(|&c| matches!(code[c].op, 21 | 22) && succ(code, c).contains(&(i + 1)));
                let Some(c) = exit else {
                    bump(
                        &mut idioms,
                        "backward jumps without an exit test to just after them",
                    );
                    continue;
                };
                bump(&mut idioms, format!("loop exit test by op {}", code[c].op));
                let Some(t) = name(&code[c].args[0]) else {
                    continue;
                };
                let Some(cmp) = code[k..c]
                    .iter()
                    .rev()
                    .find(|x| (15..=19).contains(&x.op) && name(&x.args[0]) == Some(t))
                else {
                    continue;
                };
                // An array index inside the body by one compare operand,
                // the other operand being that array's length.
                let ops2 = [name(&cmp.args[1]), name(&cmp.args[2])];
                for body in &code[c + 1..i] {
                    let (arr, idx) = match body.op {
                        32 => (name(&body.args[1]), name(&body.args[2])),
                        33 => (name(&body.args[0]), name(&body.args[1])),
                        _ => continue,
                    };
                    let (Some(arr), Some(idx)) = (arr, idx) else {
                        continue;
                    };
                    let pos = ops2.iter().position(|o| *o == Some(idx));
                    let Some(pos) = pos else { continue };
                    let other = ops2[1 - pos];
                    let is_len = code[..c].iter().any(|x| {
                        x.op == 31 && name(&x.args[0]) == other && name(&x.args[1]) == Some(arr)
                    });
                    if !is_len {
                        continue;
                    }
                    let step = code[c + 1..i]
                        .iter()
                        .find(|x| {
                            matches!(x.op, 1 | 3)
                                && name(&x.args[0]) == Some(idx)
                                && name(&x.args[1]) == Some(idx)
                        })
                        .map(|x| format!("op {} by {:?}", x.op, x.args[2]))
                        .unwrap_or_else(|| "no update op 1 or 3 of the index".into());
                    bump(
                        &mut idioms,
                        format!(
                            "array loop: exit by op {} on op {} with the index in slot {}; index update {step}",
                            code[c].op,
                            cmp.op,
                            pos + 1
                        ),
                    );
                    if cmp.op == 17 || cmp.op == 19 {
                        writeln!(
                            names,
                            "array loop guard op {} at {}.{}.{} #{c}",
                            cmp.op, s.name, f.state, f.name
                        )?;
                    }
                    break;
                }
            }
            // ::State.
            for (k, a) in ins.args.iter().enumerate() {
                if name(a) == Some("::state") {
                    let w = dest(ins.op) == Some(k);
                    bump(
                        &mut idioms,
                        format!(
                            "::State {} by op {:02}",
                            if w { "written" } else { "read" },
                            ins.op
                        ),
                    );
                    writeln!(
                        names,
                        "::State at {}.{}.{} op {}",
                        s.name, f.state, f.name, ins.op
                    )?;
                }
            }
        }
        // Functions whose end is reachable without opcode 26.
        if f.return_type != "none" && !f.native() && !f.code.is_empty() {
            let mut seen = vec![false; code.len() + 1];
            let mut work = vec![0];
            while let Some(i) = work.pop() {
                if i > code.len() || seen[i] {
                    continue;
                }
                seen[i] = true;
                // Opcode 26 leaves the function: it does not reach the end.
                if i < code.len() && code[i].op != 26 {
                    work.extend(succ(code, i));
                }
            }
            bump(&mut idioms, format!("functions returning a value whose end is reachable past the last instruction: {}", seen[code.len()]));
        }
    }
    for s in &scripts {
        for v in &s.variables {
            if matches!(v.type_name.as_str(), "int" | "float" | "bool" | "string") {
                bump(
                    &mut idioms,
                    format!(
                        "variables of a simple type storing none: {}",
                        v.init == Lit::None
                    ),
                );
            }
        }
    }
    let declared_state = scripts
        .iter()
        .filter(|s| s.variable("::state").is_some())
        .count();
    add(
        &mut idioms,
        "scripts declaring a variable named ::State",
        declared_state as u64,
    );

    // ---- B execution -------------------------------------------------------
    let mut memo = HashMap::new();
    let mut pure_funcs = Vec::new();
    for (s, f) in &funcs {
        if f.code.is_empty() || !f.state.is_empty() || f.name.contains('.') {
            continue;
        }
        if f.params.iter().all(|p| {
            let b = p.1.strip_suffix("[]").unwrap_or(&p.1);
            matches!(b, "int" | "float" | "bool" | "string")
        }) && pure(
            &lib,
            (!f.global()).then_some(s.name.as_str()),
            f,
            &mut memo,
            0,
        ) {
            pure_funcs.push((s.clone(), f.clone()));
        }
    }
    add(
        &mut counts,
        "functions statically pure with simple parameters (run)",
        pure_funcs.len() as u64,
    );
    let mut exec = Counts::new();
    let mut changed = Counts::new();
    let mut candidate_results: HashMap<(usize, usize), String> = HashMap::new();
    for variant in Variant::ALL {
        let mut host = NoHost;
        let mut vm = Vm::new(&mut lib, &mut host);
        vm.variant = variant;
        vm.limits.steps = 50_000;
        for (fi, (s, f)) in pure_funcs.iter().enumerate() {
            // Argument tuples: mixed-radix over the samples, at most 24.
            let mut lists = Vec::new();
            for p in &f.params {
                lists.push(samples(&mut vm, &p.1).unwrap_or_default());
            }
            let total: usize = lists
                .iter()
                .map(|l| l.len().max(1))
                .product::<usize>()
                .min(24);
            let mut all_ok = true;
            for n in 0..total {
                let mut k = n;
                let mut args = Vec::new();
                for l in &lists {
                    args.push(l[k % l.len()].clone());
                    k /= l.len();
                }
                let inst = if f.global() {
                    None
                } else {
                    let o = vm.world.object("self", None, &[]);
                    match vm.instantiate(Some(o), &s.name) {
                        Ok(i) => Some(i),
                        Err(_) => {
                            bump(&mut exec, format!("{variant:?}: instance refused"));
                            continue;
                        }
                    }
                };
                let o = vm.run(inst, f.clone(), args);
                let class = outcome_class(&o);
                if class != "completed" && class != "completed with an open result" {
                    all_ok = false;
                    let e = match &o {
                        Outcome::Failed(e) => e.to_string(),
                        Outcome::Blocked(b) => b.on.text(),
                        _ => String::new(),
                    };
                    writeln!(
                        names,
                        "{variant:?} run {class}: {}.{} input {n}: {e}",
                        s.name, f.name
                    )?;
                }
                bump(&mut exec, format!("{variant:?}: runs {class}"));
                let text = match &o {
                    Outcome::Completed(v) => format!("= {}", vm.world.render(v)),
                    other => class.clone() + &format!("{:?}", std::mem::discriminant(other)),
                };
                if variant == Variant::Candidate {
                    candidate_results.insert((fi, n), text);
                } else if let Some(c) = candidate_results.get(&(fi, n)) {
                    bump(
                        &mut changed,
                        format!(
                            "{variant:?}: result differs from the candidate's: {}",
                            *c != text
                        ),
                    );
                }
            }
            bump(
                &mut exec,
                format!("{variant:?}: functions with every run completed: {all_ok}"),
            );
        }
    }

    // ---- output ------------------------------------------------------------
    let mut out = String::new();
    writeln!(out, "# vm_checks: counts only (spec/vm.md, Checks)")?;
    for (k, v) in &counts {
        writeln!(out, "{v}\t{k}")?;
    }
    writeln!(
        out,
        "\n# A1 slot types under the candidate meaning: holds, fails"
    )?;
    for (k, [h, f]) in &types {
        writeln!(out, "{h}\t{f}\t{k}")?;
    }
    writeln!(out, "\n# A2 data flow: reads of temporaries not written on every path before them, per destination reading")?;
    for (k, v) in &flow {
        if *v > 0 {
            writeln!(out, "{v}\t{k}")?;
        } else {
            writeln!(out, "{k}")?;
        }
    }
    writeln!(
        out,
        "\n# A2 temporaries written by the candidate destination and read later on some path"
    )?;
    for (k, v) in &dead {
        writeln!(out, "{v}\t{k}")?;
    }
    writeln!(out, "\n# A3 idioms")?;
    for (k, v) in &idioms {
        writeln!(out, "{v}\t{k}")?;
    }
    writeln!(out, "\n# B execution of statically pure functions (at most 24 argument tuples each), per reading")?;
    for (k, v) in &exec {
        writeln!(out, "{v}\t{k}")?;
    }
    for (k, v) in &changed {
        writeln!(out, "{v}\t{k}")?;
    }
    writeln!(
        out,
        "\nwall seconds\t{:.1}",
        started.elapsed().as_secs_f64()
    )?;
    std::fs::create_dir_all(&args[1])?;
    std::fs::write(Path::new(&args[1]).join("vm-checks.txt"), out)?;
    std::fs::write(Path::new(&args[1]).join("vm-checks-names.txt"), names)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
