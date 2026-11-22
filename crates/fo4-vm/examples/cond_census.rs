#![forbid(unsafe_code)]
//! Census of every condition (CTDA, with its CIS1 and CIS2) in every
//! winning record of the load order, read with `fo4_plugin::condition`
//! (`spec/conditions.md`), and of what `fo4_vm::cond` evaluates of them at
//! the start of a new game before any script runs.
//!
//! ```text
//! cond_census MANIFEST OUT.txt
//! ```
//!
//! Writes counts only: record type tags, function indexes, field values
//! that are small codes, histograms. No name, FormID, text or editor ID.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{
    actor::Context,
    condition::{self, flag, Comparison, Ctda, List},
    quest::Quest,
    typed::Store,
    FormKey,
};
use fo4_vm::cond::{self, Combine, StaticFacts, Truth};
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
fn tag(t: &[u8; 4]) -> String {
    String::from_utf8_lossy(t).into_owned()
}
fn bucket(n: usize) -> &'static str {
    match n {
        0 => "0",
        1 => "1",
        2 => "2",
        3 => "3",
        4 => "4",
        5..=9 => "5-9",
        10..=19 => "10-19",
        _ => ">=20",
    }
}

/// What a stored word names when read as a FormID.
fn kind_of(store: &Store, ctx: Context<'_>, raw: u32, file: &str) -> String {
    if raw == 0 {
        return "zero".into();
    }
    match condition::form(ctx, raw, file, 0) {
        Err(_) => "not a FormID (master index outside the plugin's masters)".into(),
        Ok(None) => "zero".into(),
        Ok(Some(k)) => match store.signature(&k) {
            Some(s) => tag(&s),
            None => "no winning record".into(),
        },
    }
}

/// Per function index: counts by what each parameter word names.
#[derive(Default)]
struct Fn {
    total: u64,
    hosts: Counts,
    p1: Counts,
    p2: Counts,
    run_on: Counts,
    ops: Counts,
    flags: Counts,
    cis: Counts,
    value: Counts,
}

#[derive(Default)]
struct Census {
    c: Counts,
    fns: BTreeMap<u16, Fn>,
    /// Over lists evaluated in the new-game state: (scope, function) ->
    /// lists it leaves undetermined.
    undetermined: BTreeMap<(String, u16), u64>,
    /// Stage indexes of quests, read when first needed.
    stages: BTreeMap<FormKey, Option<BTreeSet<i64>>>,
}

impl Census {
    fn condition(
        &mut self,
        store: &Store,
        ctx: Context<'_>,
        host: &str,
        file: &str,
        c: &condition::Condition,
    ) {
        let d: &Ctda = &c.ctda;
        let k = &mut self.c;
        bump(k, format!("conditions by host record type\t{host}"));
        bump(
            k,
            format!("byte 0 operator code (bits 5-7)\t{}", d.operator()),
        );
        bump(k, format!("byte 0 flag bits (0-4)\t0x{:02x}", d.flags()));
        for b in 0..5 {
            if d.flags() & (1 << b) != 0 {
                bump(k, format!("byte 0 flag bit set\t0x{:02x}", 1u8 << b));
            }
        }
        bump(
            k,
            format!(
                "bytes 1-3\t{}",
                if d.unused == [0; 3] {
                    "zero"
                } else {
                    "not zero"
                }
            ),
        );
        bump(
            k,
            format!(
                "bytes 10-11\t{}",
                if d.pad == [0; 2] { "zero" } else { "not zero" }
            ),
        );
        // The comparison word, by the GLOBAL bit, and the other reading.
        let as_form = kind_of(store, ctx, d.value, file);
        let fl = f32::from_bits(d.value);
        let as_float = if !fl.is_finite() {
            "not finite"
        } else if fl.fract() == 0.0 {
            "finite, integral"
        } else {
            "finite, with a fraction"
        };
        match d.comparison() {
            Comparison::Global(_) => {
                bump(
                    k,
                    format!("comparison word, global bit set, as a FormID\t{as_form}"),
                );
            }
            Comparison::Float(_) => {
                bump(
                    k,
                    format!("comparison word, global bit clear, as an f32\t{as_float}"),
                );
                bump(
                    k,
                    format!(
                        "comparison word, global bit clear, as a FormID\t{}",
                        if as_form == "GLOB" {
                            "GLOB"
                        } else {
                            "not a GLOB"
                        }
                    ),
                );
            }
        }
        bump(k, format!("run-on word\t{}", d.run_on));
        let r = if d.reference == 0 {
            "zero".to_string()
        } else {
            let k = kind_of(store, ctx, d.reference, file);
            if k == "no winning record" && d.reference & 0x00ff_ffff < 0x800 {
                "no winning record, object id below 0x800".into()
            } else {
                k
            }
        };
        bump(
            k,
            format!("reference word by run-on word\t{}\t{r}", d.run_on),
        );
        let p3 = match d.param3 {
            -1 => "-1".to_string(),
            0 => "0".to_string(),
            n if n < 0 => "below -1".to_string(),
            n if n < 0x800 => bucket(n as usize).to_string(),
            n => format!(
                "0x800 or more, as a FormID: {}",
                kind_of(store, ctx, n as u32, file)
            ),
        };
        bump(
            k,
            format!("last word (i32) by run-on word\t{}\t{p3}", d.run_on),
        );
        bump(
            k,
            format!(
                "texts\t{}",
                match (&c.cis1, &c.cis2) {
                    (None, None) => "none",
                    (Some(_), None) => "CIS1",
                    (None, Some(_)) => "CIS2",
                    (Some(_), Some(_)) => "CIS1 and CIS2",
                }
            ),
        );
        // The stage parameters of the quest functions, against the quest.
        if matches!(d.function, 58 | 59) {
            let q = condition::form(ctx, d.param1, file, 0).ok().flatten();
            let stages = q.and_then(|q| {
                self.stages
                    .entry(q.clone())
                    .or_insert_with(|| {
                        let b = Budget::new(1 << 30);
                        fo4_vm::quest::QuestInfo::load(store, &q, &b)
                            .ok()
                            .flatten()
                            .map(|i| i.stages.iter().map(|s| i64::from(s.stage)).collect())
                    })
                    .clone()
            });
            let what = match d.function {
                58 => match d.comparison() {
                    Comparison::Float(x) if x.fract() == 0.0 => Some(x as i64),
                    _ => None,
                },
                _ => Some(i64::from(d.param2)),
            };
            let r = match (stages, what) {
                (None, _) => "parameter 1 names no readable QUST",
                (_, None) => "not an integer",
                (Some(s), Some(n)) if s.contains(&n) => "a stage of the quest",
                (Some(_), Some(_)) => "not a stage of the quest",
            };
            let which = if d.function == 58 {
                "comparison value"
            } else {
                "parameter 2"
            };
            bump(
                &mut self.c,
                format!(
                    "function {} {which} against parameter 1's stages\t{r}",
                    d.function
                ),
            );
        }
        if let Some(t) = &c.cis2 {
            let shape = match (t.starts_with(b"::"), t.ends_with(b"_var")) {
                (true, true) => "begins with :: and ends with _var",
                (true, false) => "begins with ::",
                (false, true) => "ends with _var",
                (false, false) => "other",
            };
            bump(
                &mut self.c,
                format!("CIS2 text shape by function\t{}\t{shape}", d.function),
            );
        }
        let f = self.fns.entry(d.function).or_default();
        f.total += 1;
        bump(&mut f.hosts, host);
        let p = |raw: u32| {
            let kk = kind_of(store, ctx, raw, file);
            if kk == "zero" || kk.starts_with("not a") || kk == "no winning record" {
                // Small numbers, for parameters that are not FormIDs.
                if raw < 0x800 && raw != 0 {
                    return "a number below 0x800".to_string();
                }
            }
            kk
        };
        bump(&mut f.p1, p(d.param1));
        bump(&mut f.p2, p(d.param2));
        bump(&mut f.run_on, d.run_on.to_string());
        bump(&mut f.ops, d.operator().to_string());
        bump(&mut f.flags, format!("0x{:02x}", d.flags() & !flag::OR));
        if c.cis1.is_some() {
            bump(&mut f.cis, "CIS1");
        }
        if c.cis2.is_some() {
            bump(&mut f.cis, "CIS2");
        }
        let v = match d.comparison() {
            Comparison::Global(_) => "a global",
            Comparison::Float(x) => {
                if x == 0.0 {
                    "0"
                } else if x == 1.0 {
                    "1"
                } else if x.fract() == 0.0 {
                    "another integer"
                } else {
                    "a fraction"
                }
            }
        };
        bump(&mut f.value, v);
    }

    fn list(&mut self, host: &str, l: &List) {
        let k = &mut self.c;
        bump(k, format!("lists by host record type\t{host}"));
        bump(
            k,
            format!("conditions per list\t{}", bucket(l.conditions.len())),
        );
        bump(
            k,
            format!(
                "tag before a list\t{host}\t{}",
                l.before.as_ref().map(tag).unwrap_or("none".into())
            ),
        );
        // AND/OR structure: groups of conditions joined by OR.
        let n = l.conditions.len();
        let joins: Vec<bool> = l.conditions.iter().map(|c| c.ctda.or()).collect();
        let last_or = joins.last().copied().unwrap_or(false);
        bump(
            k,
            format!(
                "last condition of a list carries the OR bit\t{}",
                if last_or { "yes" } else { "no" }
            ),
        );
        let inner = &joins[..n.saturating_sub(1)];
        let ors = inner.iter().filter(|x| **x).count();
        let ands = inner.len() - ors;
        let shape = match (ands, ors) {
            (0, 0) => "one condition",
            (_, 0) => "AND only",
            (0, _) => "OR only",
            _ => "AND and OR mixed",
        };
        bump(k, format!("list connectors\t{shape}"));
        let mut run = 1usize;
        for (i, j) in inner.iter().enumerate() {
            if *j {
                run += 1;
            }
            if !*j || i + 1 == inner.len() {
                if run > 1 {
                    bump(k, format!("OR group length (conditions)\t{}", bucket(run)));
                }
                run = 1;
            }
        }
    }
}

fn run() -> Result<(), E> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 2 {
        return Err("cond_census MANIFEST OUT.txt".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let winners: Vec<(FormKey, [u8; 4])> = store
        .winners()
        .filter(|w| !w.deleted)
        .map(|w| {
            (
                store.overrides().form_key(w.key, store.order()),
                w.signature,
            )
        })
        .collect();
    let mut cs = Census::default();
    let mut facts = StaticFacts::new_game(&store, m.limits.max_memory);
    for (key, sig) in &winners {
        let host = tag(sig);
        let b = Budget::new(m.limits.max_memory);
        bump(&mut cs.c, format!("winning records read\t{host}"));
        let Some((r, plugin)) = store.raw_with_plugin(key, &b)? else {
            bump(&mut cs.c, "winning records not read");
            continue;
        };
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        // Every CTDA by size, before any reading.
        let mut any = false;
        for s in r.subrecords.iter() {
            match &s.signature {
                b"CTDA" => {
                    any = true;
                    bump(&mut cs.c, format!("CTDA size (bytes)\t{}", r.data(s).len()));
                }
                b"CIS1" | b"CIS2" => {
                    bump(&mut cs.c, format!("subrecords\t{}", tag(&s.signature)));
                }
                b"CITC" => bump(&mut cs.c, format!("CITC in\t{host}")),
                _ => {}
            }
        }
        if !any {
            continue;
        }
        bump(&mut cs.c, format!("records with conditions\t{host}"));
        let lists = match condition::lists(&r, &b) {
            Ok(l) => l,
            Err(e) => {
                bump(&mut cs.c, format!("read error\t{host}\t{}", e.code));
                continue;
            }
        };
        // CITC before a list: does it count the list?
        for l in &lists {
            if l.before == Some(*b"CITC") {
                let s = &r.subrecords[l.first - 1];
                let d = r.data(s);
                let ok = d.len() == 4
                    && u32::from_le_bytes([d[0], d[1], d[2], d[3]]) as usize == l.conditions.len();
                bump(
                    &mut cs.c,
                    format!(
                        "CITC before a list\t{}",
                        if ok {
                            "4 bytes, equal to the list's conditions"
                        } else {
                            "differs"
                        }
                    ),
                );
            }
        }
        let file = r.source.file.clone();
        for l in &lists {
            cs.list(&host, l);
            for c in &l.conditions {
                cs.condition(&store, ctx, &host, &file, c);
            }
            // Evaluated in the new-game state before any script runs.
            let t = cond::eval_list(&l.conditions, ctx, &file, &mut facts, Combine::OrGroups);
            let alt = cond::eval_list(&l.conditions, ctx, &file, &mut facts, Combine::LeftToRight);
            bump(
                &mut cs.c,
                format!("new game, before scripts: lists\t{host}\t{}", t.word()),
            );
            bump(
                &mut cs.c,
                format!("new game, before scripts: lists (all types)\t{}", t.word()),
            );
            if t.word() != alt.word() {
                bump(
                    &mut cs.c,
                    format!(
                        "new game, before scripts: OR groups and left to right differ\t{} / {}",
                        t.word(),
                        alt.word()
                    ),
                );
            }
            if let Truth::Undetermined(fs) = &t {
                for f in fs {
                    *cs.undetermined
                        .entry(("all records".into(), f.function))
                        .or_default() += 1;
                    if host == "QUST" {
                        *cs.undetermined
                            .entry(("QUST".into(), f.function))
                            .or_default() += 1;
                    }
                }
            }
            for c in &l.conditions {
                let (v, _) = cond::eval_one(&c.ctda, ctx, &file, &mut facts);
                bump(
                    &mut cs.c,
                    format!(
                        "new game, before scripts: conditions by function\t{}\t{}",
                        c.ctda.function,
                        v.word()
                    ),
                );
            }
        }
        // Quest parts.
        if sig == b"QUST" {
            match Quest::read(&r, ctx, &b) {
                Err(e) => bump(&mut cs.c, format!("QUST read error\t{}", e.code)),
                Ok(q) => {
                    let mut part = |what: &str, list: &[fo4_plugin::quest::Condition]| {
                        if list.is_empty() {
                            return;
                        }
                        bump(&mut cs.c, format!("QUST lists by part\t{what}"));
                        for c in list {
                            let c = condition::Condition::from(c);
                            bump(
                                &mut cs.c,
                                format!(
                                    "QUST conditions by part and function\t{what}\t{}",
                                    c.ctda.function
                                ),
                            );
                        }
                    };
                    part("quest, before NEXT", &q.conditions);
                    part("quest, after NEXT", &q.event_conditions);
                    for s in &q.stages {
                        for e in &s.entries {
                            part("stage log entry", &e.conditions);
                        }
                    }
                    for o in &q.objectives {
                        for t in &o.targets {
                            part("objective target", &t.conditions);
                        }
                    }
                    for al in &q.aliases {
                        let v: Vec<fo4_plugin::quest::Condition> = al
                            .values
                            .iter()
                            .filter_map(|v| match v {
                                fo4_plugin::quest::AliasValue::Condition(c) => Some(c.clone()),
                                _ => None,
                            })
                            .collect();
                        part("alias", &v);
                    }
                    // Run-on 5's last word against the quest's aliases.
                    let ids: BTreeSet<i64> = q.aliases.iter().map(|x| i64::from(x.id)).collect();
                    for l in &lists {
                        for c in &l.conditions {
                            if c.ctda.run_on == 5 {
                                let hit = ids.contains(&i64::from(c.ctda.param3));
                                bump(
                                    &mut cs.c,
                                    format!(
                                        "QUST run-on word 5: last word names an alias of the quest\t{}",
                                        if hit { "yes" } else { "no" }
                                    ),
                                );
                            }
                            if c.ctda.flags() & flag::ALIASES != 0 {
                                let hit = ids.contains(&i64::from(c.ctda.param1));
                                bump(
                                    &mut cs.c,
                                    format!(
                                        "QUST flag 0x02 set: first parameter names an alias of the quest\t{}",
                                        if hit { "yes" } else { "no" }
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    write(Path::new(&a[1]), &cs)
}

fn line(m: &Counts) -> String {
    let mut v: Vec<(&u64, &String)> = m.iter().map(|(k, v)| (v, k)).collect();
    v.sort_by(|a, b| b.0.cmp(a.0).then(a.1.cmp(b.1)));
    v.iter()
        .map(|(n, k)| format!("{k}:{n}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn write(out: &Path, cs: &Census) -> Result<(), E> {
    let mut s = String::new();
    writeln!(
        s,
        "# cond_census: counts, record type tags and function indexes only (spec/conditions.md)"
    )?;
    writeln!(
        s,
        "# Function indexes are bare numbers: no public index-to-name table was read."
    )?;
    for (k, v) in &cs.c {
        writeln!(s, "{v}\t{k}")?;
    }
    writeln!(
        s,
        "\n## functions by index: total, host record types, operator codes, flag bits other than OR, run-on words, comparison values, texts"
    )?;
    for (f, x) in &cs.fns {
        writeln!(
            s,
            "function {f}\t{}\thosts {}\top {}\tflags {}\trun-on {}\tvalue {}\ttexts {}",
            x.total,
            line(&x.hosts),
            line(&x.ops),
            line(&x.flags),
            line(&x.run_on),
            line(&x.value),
            if x.cis.is_empty() {
                "none".into()
            } else {
                line(&x.cis)
            }
        )?;
    }
    writeln!(
        s,
        "\n## parameters by function index: what each word names read as a FormID through the record's plugin (record type of the winning record; zero; a number below 0x800 that names no winning record; no winning record; not a FormID)"
    )?;
    for (f, x) in &cs.fns {
        writeln!(s, "function {f}\tparameter 1\t{}", line(&x.p1))?;
        writeln!(s, "function {f}\tparameter 2\t{}", line(&x.p2))?;
    }
    writeln!(s, "\n## ranked by conditions: function index, conditions")?;
    let mut r: Vec<(u64, u16)> = cs.fns.iter().map(|(f, x)| (x.total, *f)).collect();
    r.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for (n, f) in r {
        writeln!(s, "{n}\tfunction {f}")?;
    }
    for scope in ["QUST", "all records"] {
        writeln!(
            s,
            "\n## new game, before scripts: lists left undetermined, by function index of an undetermined condition in them ({scope})"
        )?;
        let mut r: Vec<(u64, u16)> = cs
            .undetermined
            .iter()
            .filter(|((sc, _), _)| sc == scope)
            .map(|((_, f), n)| (*n, *f))
            .collect();
        r.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for (n, f) in r {
            writeln!(s, "{n}\tfunction {f}")?;
        }
    }
    if let Some(p) = out.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(out, s)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
