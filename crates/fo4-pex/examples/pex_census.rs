#![forbid(unsafe_code)]
//! Census of every `.pex` entry of the manifest's archives with `fo4_pex`:
//! outcomes, bytes per part, the relations between stored fields, the opcode
//! histogram with the argument kinds of each slot, the alternative arities
//! that the census rejects, and the checks behind the claimed opcode roles
//! (`spec/pex.md`). Measurement support.
//!
//! `pex_census MANIFEST OUT_DIR` writes `pex-census.txt` (counts and small
//! stored values only, the file committed as `spec/pex-census.txt`) and
//! `pex-names.txt` (the most called functions by name; never versioned).
use fo4_core::Budget;
use fo4_pex::index::{lower as ctx_lower, Frame, Index, Scope};
use fo4_pex::{
    Function, Instruction, Pex, Place, Shape, Str, Value, FUNCTION_GLOBAL, FUNCTION_NATIVE,
    OPCODES, PROPERTY_READ, PROPERTY_VARIABLE, PROPERTY_WRITE,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt::Write as _,
};

mod support;

type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}
fn add(m: &mut Counts, k: impl Into<String>, n: u64) {
    *m.entry(k.into()).or_default() += n;
}
fn check(m: &mut BTreeMap<String, [u64; 2]>, k: &str, ok: bool) {
    m.entry(k.to_string()).or_default()[usize::from(!ok)] += 1;
}
fn lc(b: &[u8]) -> String {
    String::from_utf8_lossy(b).to_ascii_lowercase()
}
fn kind_name(k: u8) -> &'static str {
    ["none", "ident", "string", "int", "float", "bool"][usize::from(k.min(5))]
}
fn bucket(n: usize) -> String {
    match n {
        0..=4 => n.to_string(),
        5..=9 => "5-9".into(),
        10..=99 => "10-99".into(),
        100..=999 => "100-999".into(),
        _ => ">=1000".into(),
    }
}

fn label(s: Scope) -> &'static str {
    match s {
        Scope::Local => "local",
        Scope::Param => "param",
        Scope::Variable => "variable",
        Scope::ParentVariable => "variable of a parent",
        Scope::SelfRef => "self",
        Scope::Other => "other",
    }
}

/// Index of the winning scripts by lower-case object name.
fn build_index(all: &[(String, Vec<u8>)]) -> Index {
    let mut ix = Index::default();
    let b = Budget::new(u64::MAX);
    for (name, bytes) in all {
        if let Ok(p) = Pex::read(bytes, name, &b) {
            ix.add(&p);
        }
    }
    ix
}

/// Declared type class used in the typing table.
fn type_class(t: &str) -> &'static str {
    match t {
        "int" => "Int",
        "float" => "Float",
        "bool" => "Bool",
        "string" => "String",
        "var" => "Var",
        "none" => "None",
        _ if t.ends_with("[]") => "array",
        _ if t.contains('#') => "struct",
        _ => "object",
    }
}

/// Every string index a file stores outside the string table and header.
fn refs(p: &Pex<'_>) -> Vec<Str> {
    let mut r = Vec::new();
    let val = |r: &mut Vec<Str>, v: &Value| {
        if let Value::Ident(i) | Value::Str(i) = v {
            r.push(*i)
        }
    };
    let func = |r: &mut Vec<Str>, f: &Function| {
        r.extend([f.return_type, f.doc]);
        for (a, b) in f.params.iter().chain(&f.locals) {
            r.extend([*a, *b]);
        }
        for i in &f.code {
            for v in i.args.iter().chain(&i.extra) {
                val(r, v);
            }
        }
    };
    if let Some(d) = &p.debug {
        for f in &d.functions {
            r.extend([f.object, f.state, f.name]);
        }
        for g in &d.property_groups {
            r.extend([g.object, g.name, g.doc]);
            r.extend(&g.properties);
        }
        for s in &d.struct_orders {
            r.extend([s.object, s.name]);
            r.extend(&s.members);
        }
    }
    for u in &p.user_flags {
        r.push(u.name);
    }
    for o in &p.objects {
        r.extend([o.name, o.parent, o.doc, o.auto_state]);
        for s in &o.structs {
            r.push(s.name);
            for m in &s.members {
                r.extend([m.name, m.type_name, m.doc]);
                val(&mut r, &m.value);
            }
        }
        for v in &o.variables {
            r.extend([v.name, v.type_name]);
            val(&mut r, &v.value);
        }
        for pr in &o.properties {
            r.extend([pr.name, pr.type_name, pr.doc]);
            r.extend(pr.variable);
            for f in pr.read.iter().chain(&pr.write) {
                func(&mut r, f);
            }
        }
        for st in &o.states {
            r.push(st.name);
            for f in &st.functions {
                r.push(f.name);
                func(&mut r, &f.function);
            }
        }
    }
    r
}

/// The alternatives tried for each opcode: every fixed arity 0..=6 and every
/// counted shape with 1..=6 fixed arguments, except the table's own.
fn alternatives(own: Shape) -> Vec<Shape> {
    let _: Option<Shape> = None;
    let mut v = Vec::new();
    for fixed in 0..=6u8 {
        for counted in [false, true] {
            if counted && fixed == 0 {
                continue;
            }
            let s = Shape { fixed, counted };
            if s != own {
                v.push(s);
            }
        }
    }
    v
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("pex_census MANIFEST OUT_DIR".into());
    }
    let started = std::time::Instant::now();
    let (m, vfs) = support::open(&args[0])?;
    let entries = support::entries(&vfs);
    let mut c = Counts::new();
    let mut checks = BTreeMap::<String, [u64; 2]>::new();
    let mut values = Counts::new();
    let mut parts = Counts::new();
    let mut winners: Vec<(String, Vec<u8>)> = Vec::new();
    let mut groups = Counts::new();
    let mut total_bytes = 0u64;
    let mut read_bytes = 0u64;
    let mut paths = BTreeSet::new();
    for e in &entries {
        let cand = &vfs.candidates[e.candidate];
        let name = String::from_utf8_lossy(cand.key.as_deref().unwrap_or_default()).into_owned();
        paths.insert(name.clone());
        let b = Budget::new(m.limits.max_memory);
        let bytes = match vfs.read_candidate(cand, &b) {
            Ok(v) => v,
            Err(err) => {
                bump(&mut c, format!("entry read error {}", err.code));
                continue;
            }
        };
        bump(
            &mut groups,
            format!(
                "{} {}",
                e.group,
                if e.winner { "winner" } else { "overridden" }
            ),
        );
        total_bytes += bytes.len() as u64;
        match Pex::read(&bytes, &name, &b) {
            Ok(p) => {
                bump(&mut c, "read to the last byte");
                read_bytes += bytes.len() as u64;
                let s = p.sections;
                add(&mut parts, "1 header", u64::from(s.strings));
                add(&mut parts, "2 string table", u64::from(s.debug - s.strings));
                add(&mut parts, "3 debug", u64::from(s.user_flags - s.debug));
                add(
                    &mut parts,
                    "4 user flags",
                    u64::from(s.objects - s.user_flags),
                );
                add(&mut parts, "5 objects", u64::from(s.end - s.objects));
            }
            Err(err) => bump(
                &mut c,
                format!("read error {} (expected {})", err.code, err.expected),
            ),
        }
        if e.winner {
            winners.push((name, bytes));
        }
    }
    let ix = build_index(&winners);

    // Per-winner checks.
    let mut ops = [0u64; 256];
    let mut op_files = [0u64; 256];
    let mut slot_kinds = Counts::new();
    let mut slot_scope = Counts::new();
    let mut slot_types = Counts::new();
    let mut extra_len = Counts::new();
    let mut code_bytes = 0u64;
    let mut instructions = 0u64;
    let mut functions = 0u64;
    let mut flags_tab = Counts::new();
    let mut flag_bits: HashMap<String, u8> = HashMap::new();
    let mut flag_use = Counts::new();
    let mut calls_native: HashMap<String, u64> = HashMap::new();
    let mut compiled = (u64::MAX, 0u64);
    let mut users = HashSet::new();
    let mut machines = HashSet::new();
    let b = Budget::new(u64::MAX);
    for (name, bytes) in &winners {
        let Ok(p) = Pex::read(bytes, name, &b) else {
            continue;
        };
        let h = &p.header;
        compiled = (compiled.0.min(h.compiled), compiled.1.max(h.compiled));
        users.insert(h.user.to_vec());
        machines.insert(h.machine.to_vec());
        check(
            &mut checks,
            "header source text ends .psc",
            lc(h.source).ends_with(".psc"),
        );
        let stem = lc(h.source)
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or_default()
            .trim_end_matches(".psc")
            .to_string();
        if let Some(o) = p.objects.first() {
            let on = lc(p.str(o.name));
            let last = on.rsplit(':').next().unwrap_or_default().to_string();
            check(
                &mut checks,
                "header source file stem == object name after its last ':'",
                stem == last,
            );
            check(&mut checks, "string 0 == object name", o.name == 0);
            let segs = on.split(':').count() - 1;
            bump(&mut values, format!("object name ':' separators {segs}"));
            let path_ns = name.trim_start_matches("scripts/").matches('/').count();
            check(
                &mut checks,
                "object name ':' separators == folders below scripts/ in the entry path",
                segs == path_ns,
            );
            let tail = name
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .trim_end_matches(".pex");
            check(
                &mut checks,
                "entry file stem == object name after its last ':'",
                tail == last,
            );
        }
        bump(&mut values, format!("objects per file {}", p.objects.len()));
        bump(
            &mut values,
            format!("strings per file {}", bucket(p.strings.len())),
        );
        let distinct: HashSet<String> = p
            .strings
            .iter()
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        check(
            &mut checks,
            "strings distinct (bytes) within a file",
            distinct.len() == p.strings.len(),
        );
        let ascii = p.strings.iter().all(|s| s.is_ascii());
        check(&mut checks, "every string ASCII", ascii);
        let r = refs(&p);
        let used: HashSet<Str> = r.iter().copied().collect();
        let unused = p.strings.len() - used.len();
        check(
            &mut checks,
            "every string referenced by a field",
            unused == 0,
        );
        add(
            &mut values,
            "strings never referenced (total)",
            unused as u64,
        );
        add(&mut values, "strings (total)", p.strings.len() as u64);

        // User flags.
        let mut declared = 0u32;
        for u in &p.user_flags {
            let n = lc(p.str(u.name));
            bump(&mut flags_tab, format!("{n} bit {}", u.bit));
            let prev = *flag_bits.entry(n.clone()).or_insert(u.bit);
            check(
                &mut checks,
                "user flag name keeps one bit across files",
                prev == u.bit,
            );
            check(&mut checks, "user flag bit < 32", u.bit < 32);
            declared |= 1u32.checked_shl(u.bit.into()).unwrap_or(0);
        }
        // Debug info.
        let sites = p.functions();
        bump(
            &mut values,
            format!("debug presence {}", u8::from(p.debug.is_some())),
        );
        if let Some(d) = &p.debug {
            check(
                &mut checks,
                "debug modified time <= header compiled time",
                d.modified <= h.compiled,
            );
            // Pairing of debug entries with stored functions: first at the
            // place the entry names with the same instruction count, then the
            // same name in another state with the same count.
            let mut covered = HashSet::new();
            for df in &d.functions {
                let same_object = |s: &&fo4_pex::FunctionSite<'_>| {
                    lc(p.str(p.objects[s.object].name)) == lc(p.str(df.object))
                };
                let at_place = |s: &fo4_pex::FunctionSite<'_>| match (df.kind, s.place) {
                    (0, Place::State(st, n)) => st == df.state && n == df.name,
                    (1, Place::Read(n)) | (2, Place::Write(n)) => n == df.name,
                    _ => false,
                };
                let named = |s: &fo4_pex::FunctionSite<'_>| match s.place {
                    Place::State(_, n) => df.kind == 0 && n == df.name,
                    _ => false,
                };
                let count = |s: &fo4_pex::FunctionSite<'_>| s.function.code.len() == df.lines.len();
                let pick = |f: &dyn Fn(&fo4_pex::FunctionSite<'_>) -> bool| {
                    sites
                        .iter()
                        .enumerate()
                        .find(|(k, s)| same_object(s) && f(s) && !covered.contains(k))
                        .map(|(k, _)| k)
                };
                let how = if let Some(k) = pick(&|s| at_place(s) && count(s)) {
                    covered.insert(k);
                    "at the place it names, same instruction count"
                } else if let Some(k) = pick(&|s| named(s) && count(s)) {
                    covered.insert(k);
                    "same name in another state, same instruction count"
                } else if sites.iter().any(|s| same_object(&s) && at_place(s)) {
                    "a function at the place it names, other instruction count"
                } else {
                    "no function of that name and count"
                };
                bump(
                    &mut values,
                    format!("debug function kind {} paired: {how}", df.kind),
                );
            }
            for (k, s) in sites.iter().enumerate() {
                let native = s.function.flags & FUNCTION_NATIVE != 0;
                bump(
                    &mut values,
                    format!(
                        "debug present: {} function paired with an entry: {}",
                        if native { "native" } else { "non-native" },
                        covered.contains(&k)
                    ),
                );
            }
            for g in &d.property_groups {
                uflags(
                    &mut checks,
                    &mut flag_use,
                    &p,
                    declared,
                    "property group",
                    g.user_flags,
                );
                let o = p.objects.iter().find(|o| o.name == g.object);
                check(
                    &mut checks,
                    "property group object is the file's object",
                    o.is_some(),
                );
                if let Some(o) = o {
                    for n in &g.properties {
                        check(
                            &mut checks,
                            "property group lists properties of its object",
                            o.properties.iter().any(|x| x.name == *n),
                        );
                    }
                }
            }
            for o in &p.objects {
                let listed: Vec<Str> = d
                    .property_groups
                    .iter()
                    .filter(|g| g.object == o.name)
                    .flat_map(|g| g.properties.iter().copied())
                    .collect();
                for pr in &o.properties {
                    let n = listed.iter().filter(|x| **x == pr.name).count();
                    check(
                        &mut checks,
                        "debug present: each property listed in exactly one group",
                        n == 1,
                    );
                }
                for st in &o.structs {
                    let order = d
                        .struct_orders
                        .iter()
                        .find(|x| x.object == o.name && x.name == st.name);
                    check(
                        &mut checks,
                        "debug present: each struct has an order entry",
                        order.is_some(),
                    );
                    if let Some(order) = order {
                        let stored: Vec<Str> = st.members.iter().map(|m| m.name).collect();
                        check(
                            &mut checks,
                            "struct order lists the stored members in stored order",
                            order.members == stored,
                        );
                        let mut a = order.members.clone();
                        let mut b2 = stored.clone();
                        a.sort_unstable();
                        b2.sort_unstable();
                        check(
                            &mut checks,
                            "struct order lists the stored members (as a set)",
                            a == b2,
                        );
                    }
                }
            }
            add(
                &mut values,
                "debug function entries",
                d.functions.len() as u64,
            );
            add(
                &mut values,
                "debug property groups",
                d.property_groups.len() as u64,
            );
            add(
                &mut values,
                "debug struct orders",
                d.struct_orders.len() as u64,
            );
            for df in &d.functions {
                bump(&mut values, format!("debug function kind {}", df.kind));
            }
        }

        for (oi, o) in p.objects.iter().enumerate() {
            uflags(
                &mut checks,
                &mut flag_use,
                &p,
                declared,
                "object",
                o.user_flags,
            );
            bump(&mut values, format!("object const byte {}", o.constant));
            bump(
                &mut values,
                format!(
                    "object const byte {} and debug present: {}",
                    o.constant,
                    p.debug.is_some()
                ),
            );
            bump(
                &mut values,
                format!("object doc empty {}", p.str(o.doc).is_empty()),
            );
            let parent = lc(p.str(o.parent));
            if parent.is_empty() {
                bump(&mut values, "object parent empty");
            } else {
                check(
                    &mut checks,
                    "object parent names a winning script",
                    ix.scripts.contains_key(&parent),
                );
            }
            check(
                &mut checks,
                "object auto state names one of its states",
                o.states.iter().any(|s| s.name == o.auto_state),
            );
            check(
                &mut checks,
                "object has an empty-named state",
                o.states.iter().any(|s| p.str(s.name).is_empty()),
            );
            bump(
                &mut values,
                format!("object auto state empty {}", p.str(o.auto_state).is_empty()),
            );
            bump(
                &mut values,
                format!("states per object {}", bucket(o.states.len())),
            );
            add(&mut values, "structs", o.structs.len() as u64);
            for st in &o.structs {
                add(&mut values, "struct members", st.members.len() as u64);
                for mb in &st.members {
                    uflags(
                        &mut checks,
                        &mut flag_use,
                        &p,
                        declared,
                        "struct member",
                        mb.user_flags,
                    );
                    bump(
                        &mut values,
                        format!("struct member value kind {}", kind_name(mb.value.kind())),
                    );
                    bump(
                        &mut values,
                        format!("struct member const byte {}", mb.constant),
                    );
                    let t = lc(p.str(mb.type_name));
                    check(
                        &mut checks,
                        "member/variable value kind none or the declared simple type",
                        value_fits(&mb.value, &t),
                    );
                }
            }
            add(&mut values, "variables", o.variables.len() as u64);
            for v in &o.variables {
                uflags(
                    &mut checks,
                    &mut flag_use,
                    &p,
                    declared,
                    "variable",
                    v.user_flags,
                );
                bump(
                    &mut values,
                    format!("variable value kind {}", kind_name(v.value.kind())),
                );
                bump(&mut values, format!("variable const byte {}", v.constant));
                let backs = o.properties.iter().any(|pr| pr.variable == Some(v.name));
                bump(
                    &mut values,
                    format!(
                        "variable const byte {} and a property names it: {backs}",
                        v.constant
                    ),
                );
                let t = lc(p.str(v.type_name));
                check(
                    &mut checks,
                    "member/variable value kind none or the declared simple type",
                    value_fits(&v.value, &t),
                );
            }
            add(&mut values, "properties", o.properties.len() as u64);
            for pr in &o.properties {
                uflags(
                    &mut checks,
                    &mut flag_use,
                    &p,
                    declared,
                    "property",
                    pr.user_flags,
                );
                bump(&mut values, format!("property flags {}", pr.flags));
                if pr.flags & PROPERTY_VARIABLE != 0 {
                    let v = o.variables.iter().find(|v| Some(v.name) == pr.variable);
                    check(
                        &mut checks,
                        "property variable names a variable of the object",
                        v.is_some(),
                    );
                    if let Some(v) = v {
                        check(
                            &mut checks,
                            "property variable has the property type",
                            lc(p.str(v.type_name)) == lc(p.str(pr.type_name)),
                        );
                    }
                    check(
                        &mut checks,
                        "property with bit 4 also has bits 1 and 2",
                        pr.flags & 3 == 3,
                    );
                }
                if let Some(f) = &pr.read {
                    check(
                        &mut checks,
                        "property read function: no params, returns the property type",
                        f.params.is_empty() && lc(p.str(f.return_type)) == lc(p.str(pr.type_name)),
                    );
                }
                if let Some(f) = &pr.write {
                    check(
                        &mut checks,
                        "property write function: one param of the property type, returns None",
                        f.params.len() == 1
                            && lc(p.str(f.params[0].1)) == lc(p.str(pr.type_name))
                            && lc(p.str(f.return_type)) == "none",
                    );
                }
                let _ = (PROPERTY_READ, PROPERTY_WRITE);
            }
            let empty_state: HashSet<Str> = o
                .states
                .iter()
                .filter(|s| p.str(s.name).is_empty())
                .flat_map(|s| s.functions.iter().map(|f| f.name))
                .collect();
            for st in o.states.iter().filter(|s| !p.str(s.name).is_empty()) {
                for f in &st.functions {
                    let inherited = ix.function(&parent, &lc(p.str(f.name))).is_some();
                    check(
                        &mut checks,
                        "function of a named state also in the empty state or a parent",
                        empty_state.contains(&f.name) || inherited,
                    );
                }
            }
            for site in sites.iter().filter(|s| s.object == oi) {
                let f = site.function;
                functions += 1;
                uflags(
                    &mut checks,
                    &mut flag_use,
                    &p,
                    declared,
                    "function",
                    f.user_flags,
                );
                bump(&mut values, format!("function flags {}", f.flags));
                bump(
                    &mut values,
                    format!("function params {}", bucket(f.params.len())),
                );
                bump(
                    &mut values,
                    format!("function locals {}", bucket(f.locals.len())),
                );
                bump(
                    &mut values,
                    format!("function instructions {}", bucket(f.code.len())),
                );
                if f.flags & FUNCTION_NATIVE != 0 {
                    check(
                        &mut checks,
                        "native function has no instructions and no locals",
                        f.code.is_empty() && f.locals.is_empty(),
                    );
                } else {
                    bump(
                        &mut values,
                        format!(
                            "non-native function with no instructions {}",
                            f.code.is_empty()
                        ),
                    );
                }
                if f.flags & FUNCTION_GLOBAL != 0 {
                    check(
                        &mut checks,
                        "global function stored in the empty state",
                        matches!(site.place, Place::State(s, _) if p.str(s).is_empty()),
                    );
                }
                let ctx = Frame::new(&p, oi, f, &ix);
                let n = f.code.len() as i64;
                for (k, ins) in f.code.iter().enumerate() {
                    instructions += 1;
                    ops[usize::from(ins.op)] += 1;
                    code_bytes += ins.stored_len() as u64;
                    if OPCODES[usize::from(ins.op)].is_some_and(|s| s.counted) {
                        bump(
                            &mut extra_len,
                            format!("Op{:02} counted list {}", ins.op, bucket(ins.extra.len())),
                        );
                    }
                    slots(&ctx, ins, &mut slot_kinds, &mut slot_scope, &mut slot_types);
                    for (si, v) in ins.args.iter().enumerate() {
                        if let Value::Int(t) = v {
                            let to = k as i64 + i64::from(*t);
                            check(
                                &mut checks,
                                &format!(
                                    "Op{:02} slot {si} int as relative target lands in 0..=count",
                                    ins.op
                                ),
                                (0..=n).contains(&to),
                            );
                            if (0..=n).contains(&to) {
                                check(
                                    &mut checks,
                                    &format!(
                                        "Op{:02} slot {si} int as relative target is not 0",
                                        ins.op
                                    ),
                                    *t != 0,
                                );
                            }
                        }
                    }
                    roles(&ctx, &ix, ins, &mut checks, &mut values, &mut calls_native);
                }
            }
        }
        let mut seen = [false; 256];
        for s in &sites {
            for i in &s.function.code {
                seen[usize::from(i.op)] = true;
            }
        }
        for (o, x) in seen.iter().enumerate() {
            if *x {
                op_files[o] += 1;
            }
        }
    }

    // Alternative arities.
    let mut alt = String::new();
    for op in 0..OPCODES.len() {
        let holders: Vec<&(String, Vec<u8>)> = winners
            .iter()
            .filter(|(n, bytes)| {
                Pex::read(bytes, n, &b).is_ok_and(|p| {
                    p.functions()
                        .iter()
                        .any(|s| s.function.code.iter().any(|i| usize::from(i.op) == op))
                })
            })
            .collect();
        let Some(own) = OPCODES[op] else {
            let _ = writeln!(
                alt,
                "Op{op:02}\t-\tfiles {}\tunmeasured (refused by the reader)",
                holders.len()
            );
            continue;
        };
        let mut line = format!(
            "Op{op:02}\t{}{}\tfiles {}\t",
            own.fixed,
            if own.counted { "+n" } else { "" },
            holders.len()
        );
        let mut exact_alts = Vec::new();
        let mut all = 0;
        for a in alternatives(own) {
            let mut t = OPCODES;
            t[op] = Some(a);
            let ok = holders
                .iter()
                .filter(|(n, bytes)| Pex::read_with(bytes, n, &b, &t).is_ok())
                .count();
            if ok == holders.len() {
                all += 1;
            }
            if ok > 0 {
                exact_alts.push(format!(
                    "{}{}:{ok}",
                    a.fixed,
                    if a.counted { "+n" } else { "" }
                ));
            }
        }
        let _ = write!(line, "alternatives reading every file: {all}; ");
        if exact_alts.is_empty() {
            line.push_str("every alternative fails on every file");
        } else {
            let _ = write!(
                line,
                "alternatives reading some files exactly: {}",
                exact_alts.join(" ")
            );
        }
        let _ = writeln!(alt, "{line}");
    }

    let mut out = String::new();
    let _ = writeln!(
        out,
        "pex entries (every candidate of every mounted archive)\t{}",
        entries.len()
    );
    let _ = writeln!(out, "distinct paths\t{}", paths.len());
    let _ = writeln!(out, "bytes\t{total_bytes}");
    let _ = writeln!(out, "bytes read to the last byte\t{read_bytes}");
    let _ = writeln!(out, "functions (winners)\t{functions}");
    let _ = writeln!(out, "instructions (winners)\t{instructions}");
    let _ = writeln!(out, "instruction bytes (winners)\t{code_bytes}");
    let _ = writeln!(
        out,
        "header compiled u64 min|max\t{}|{}",
        compiled.0, compiled.1
    );
    let _ = writeln!(
        out,
        "distinct header user texts|machine texts\t{}|{}",
        users.len(),
        machines.len()
    );
    let _ = writeln!(out, "wall seconds\t{:.1}", started.elapsed().as_secs_f64());
    let sec = |out: &mut String, t: &str, m: &Counts| {
        let _ = writeln!(out, "\n# {t}");
        for (k, v) in m {
            let _ = writeln!(out, "{v}\t{k}");
        }
    };
    sec(
        &mut out,
        "entries by mount group (base, dlc, other) and priority",
        &groups,
    );
    sec(&mut out, "outcomes (every entry)", &c);
    sec(&mut out, "bytes by part (every entry read)", &parts);
    let _ = writeln!(out, "\n# checks (winners): holds, fails");
    for (k, [h, f]) in &checks {
        let _ = writeln!(out, "{h}\t{f}\t{k}");
    }
    sec(&mut out, "values (winners)", &values);
    sec(
        &mut out,
        "user flag table entries (name bit), per file",
        &flags_tab,
    );
    sec(&mut out, "user flag bits set, by element", &flag_use);
    let _ = writeln!(
        out,
        "\n# opcodes (winners): opcode, arity (+n: counted list), instructions, files"
    );
    for (o, n) in ops.iter().enumerate() {
        if *n > 0 || o < OPCODES.len() {
            let s = OPCODES.get(o).copied().flatten();
            let _ = writeln!(
                out,
                "Op{o:02}\t{}\t{n}\t{}",
                s.map_or("-".into(), |s| format!(
                    "{}{}",
                    s.fixed,
                    if s.counted { "+n" } else { "" }
                )),
                op_files[o]
            );
        }
    }
    let _ = writeln!(out, "\n# alternative arities: each tried with the rest of the table fixed, over the files holding the opcode");
    out.push_str(&alt);
    sec(
        &mut out,
        "argument kinds by opcode and slot (x: counted list)",
        &slot_kinds,
    );
    sec(
        &mut out,
        "identifier arguments by opcode, slot and what they name",
        &slot_scope,
    );
    sec(
        &mut out,
        "declared type class of identifier arguments naming a local, param, variable or self",
        &slot_types,
    );
    sec(&mut out, "counted list lengths", &extra_len);
    std::fs::create_dir_all(&args[1])?;
    std::fs::write(std::path::Path::new(&args[1]).join("pex-census.txt"), out)?;
    let mut names: Vec<(u64, String)> = calls_native.into_iter().map(|(k, v)| (v, k)).collect();
    names.sort_by(|a, b| b.cmp(a));
    let mut nm =
        String::from("# calls resolved to a native function, by callee (script.function)\n");
    for (v, k) in names.iter().take(300) {
        let _ = writeln!(nm, "{v}\t{k}");
    }
    std::fs::write(std::path::Path::new(&args[1]).join("pex-names.txt"), nm)?;
    Ok(())
}

fn uflags(
    checks: &mut BTreeMap<String, [u64; 2]>,
    flag_use: &mut Counts,
    p: &Pex<'_>,
    declared: u32,
    what: &str,
    v: u32,
) {
    check(
        checks,
        "user flag words use declared bits only",
        v & !declared == 0,
    );
    for bit in 0..32u32 {
        if v & (1 << bit) != 0 {
            let n = p
                .user_flags
                .iter()
                .find(|u| u32::from(u.bit) == bit)
                .map(|u| lc(p.str(u.name)))
                .unwrap_or_else(|| format!("bit {bit}"));
            bump(flag_use, format!("{what}: {n}"));
        }
    }
}

fn value_fits(v: &Value, t: &str) -> bool {
    matches!(
        (v, t),
        (Value::None, _)
            | (Value::Int(_), "int")
            | (Value::Float(_), "float")
            | (Value::Bool(_), "bool")
            | (Value::Str(_), "string")
    )
}

fn slots(
    ctx: &Frame<'_>,
    ins: &Instruction,
    kinds: &mut Counts,
    scope: &mut Counts,
    types: &mut Counts,
) {
    let all = ins
        .args
        .iter()
        .map(|v| (false, v))
        .chain(ins.extra.iter().map(|v| (true, v)));
    for (si, (x, v)) in all.enumerate() {
        let slot = if x { "x".to_string() } else { si.to_string() };
        bump(
            kinds,
            format!("Op{:02} slot {slot} {}", ins.op, kind_name(v.kind())),
        );
        if let Some((s, t)) = ctx.ident(v) {
            bump(scope, format!("Op{:02} slot {slot} {}", ins.op, label(s)));
            if s != Scope::Other {
                bump(
                    types,
                    format!("Op{:02} slot {slot} {}", ins.op, type_class(&t)),
                );
            }
        }
    }
}

/// The checks behind the claimed roles: name slots resolved against the
/// winning scripts' declarations.
fn roles(
    ctx: &Frame<'_>,
    ix: &Index,
    ins: &Instruction,
    checks: &mut BTreeMap<String, [u64; 2]>,
    values: &mut Counts,
    natives: &mut HashMap<String, u64>,
) {
    let a = &ins.args;
    let tag = match ins.op {
        23 => "Op23 (name, receiver, destination, n)",
        24 => "Op24 (name, destination, n) from the parent",
        25 => "Op25 (script, name, destination, n)",
        _ => "",
    };
    match ins.op {
        23..=25 => {
            let ty = match ins.op {
                23 => a.get(1).and_then(|v| ctx.ident(v)).map(|(_, t)| t),
                24 => ix.scripts.get(ctx.object()).map(|s| s.parent.clone()),
                _ => a.first().and_then(|v| ctx.name(v)),
            };
            let Some(ty) = ty else {
                bump(
                    values,
                    format!("{tag}: receiver or script slot not an identifier"),
                );
                return;
            };
            let tc = type_class(&ty);
            if tc != "object" {
                bump(values, format!("{tag}: receiver of type class {tc}"));
                return;
            }
            if !ix.scripts.contains_key(&ty) {
                bump(values, format!("{tag}: receiver type not a winning script"));
                return;
            }
            let dest = a.get(if ins.op == 24 { 1 } else { 2 });
            let global = ins.op == 25;
            let resolves = format!("{tag}: name resolves to a function of the type or a parent");
            match ctx.callee(ix, ins) {
                None => check(checks, &resolves, false),
                Some(c) => {
                    check(checks, &resolves, true);
                    let f = c.decl;
                    check(
                        checks,
                        &format!("{tag}: counted list length == callee params"),
                        f.params == ins.extra.len(),
                    );
                    check(
                        checks,
                        &format!("{tag}: callee global flag == {global}"),
                        f.global() == global,
                    );
                    if f.native() {
                        *natives
                            .entry(format!("{}.{}", c.script, c.name))
                            .or_default() += 1;
                        bump(values, format!("{tag}: callee native"));
                    } else {
                        bump(values, format!("{tag}: callee scripted"));
                    }
                    if let Some((s, t)) = dest.and_then(|d| ctx.ident(d)) {
                        if s != Scope::Other {
                            check(
                                checks,
                                &format!(
                                    "{tag}: destination slot declared type == callee return type"
                                ),
                                t == f.return_type,
                            );
                        }
                    }
                }
            }
        }
        28 | 29 => {
            let tag = format!("Op{} (name, receiver, value)", ins.op);
            let ty = a.get(1).and_then(|v| ctx.ident(v)).map(|(_, t)| t);
            let (Some(ty), Some(n)) = (ty, a.first().and_then(|v| ctx.name(v))) else {
                bump(values, format!("{tag}: receiver or name not an identifier"));
                return;
            };
            if !ix.scripts.contains_key(&ty) {
                bump(
                    values,
                    format!(
                        "{tag}: receiver type not a winning script ({})",
                        type_class(&ty)
                    ),
                );
                return;
            }
            let pt = ix.property(&ty, &n);
            check(
                checks,
                &format!("{tag}: name resolves to a property of the type or a parent"),
                pt.is_some(),
            );
            if let (Some(pt), Some((s, t))) = (pt, a.get(2).and_then(|d| ctx.ident(d))) {
                if s != Scope::Other {
                    check(
                        checks,
                        &format!("{tag}: value slot declared type == property type"),
                        pt == t,
                    );
                }
            }
        }
        36 => {
            let n = a.get(2).and_then(|v| ctx.name(v));
            let ok = n.as_ref().is_some_and(|n| {
                ix.scripts.contains_key(n)
                    || matches!(n.as_str(), "int" | "float" | "bool" | "string" | "var")
            });
            check(
                checks,
                "Op36 slot 2 names a winning script or a simple type",
                ok,
            );
        }
        38..=40 => {
            let (sv, nv) = match ins.op {
                38 => (a.get(1), a.get(2)),
                39 => (a.first(), a.get(1)),
                _ => (a.first(), a.get(2)),
            };
            let tag = format!("Op{} (struct slot, member-name slot)", ins.op);
            let Some((_, t)) = sv.and_then(|v| ctx.ident(v)) else {
                bump(values, format!("{tag}: struct slot not an identifier"));
                return;
            };
            let t = if ins.op == 40 {
                t.trim_end_matches("[]").to_string()
            } else {
                t
            };
            if !t.contains('#') {
                bump(
                    values,
                    format!("{tag}: struct slot of type class {}", type_class(&t)),
                );
                return;
            }
            let members = ix.struct_members(&t);
            check(
                checks,
                &format!("{tag}: struct slot type names a struct of a winning script"),
                members.is_some(),
            );
            let name = nv.and_then(|v| match v {
                Value::Str(i) => Some(ctx_lower(ctx.text(*i))),
                v => ctx.name(v),
            });
            if let (Some(m), Some(n)) = (members, name) {
                check(
                    checks,
                    &format!("{tag}: member-name slot names a member of that struct"),
                    m.contains(&n),
                );
            }
        }
        _ => {}
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
