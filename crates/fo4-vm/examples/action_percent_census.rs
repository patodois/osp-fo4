#![forbid(unsafe_code)]
//! Every condition of function 131 and what its parameters name, the data
//! the reading `cond_fn_131_action_percent_v1` rests on (`spec/vm.md`,
//! Scene action progress): `action_percent_census MANIFEST OUT.txt
//! NAMES.txt`.
//!
//! OUT.txt holds counts and kinds only (`spec/action-percent-census.txt`);
//! NAMES.txt names records and is never versioned. Nothing runs:
//!
//! 1. the conditions of function 131 in every winning record's condition
//!    lists, by host record type, operator code, flag bits, run-on word
//!    and comparison value;
//! 2. in scene records, by the part of the record that holds them (phase
//!    start or completion, an action's conditions, the scene's own);
//! 3. what the parameters name: whether the first is a SCEN and the host
//!    scene itself; whether the second is the index (INAM) of an action of
//!    that scene, the action's kind, and for a phase list whether the
//!    action spans that phase, ends in it, or lies in another phase.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{
    actor::Context,
    condition::{self, Comparison, Ctda},
    scen::Scene,
    typed::Store,
    FormKey,
};
use fo4_vm::{bind::record_key, scene::action_kind};
use std::{collections::BTreeMap, fmt::Write as _, path::Path};

type E = Box<dyn std::error::Error>;
type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}

const FUNCTION: u16 = 131;

fn value(c: &Ctda) -> String {
    match c.comparison() {
        Comparison::Global(_) => "a global".into(),
        Comparison::Float(v) if v.fract() == 0.0 && (0.0..=100.0).contains(&v) => {
            let b = if v == 0.0 {
                "0"
            } else if v < 50.0 {
                "1 to 49"
            } else if v < 80.0 {
                "50 to 79"
            } else if v < 100.0 {
                "80 to 99"
            } else {
                "100"
            };
            format!("an integer {b}")
        }
        Comparison::Float(_) => "another number".into(),
    }
}

fn run() -> Result<(), E> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 3 {
        return Err("action_percent_census MANIFEST OUT.txt NAMES.txt".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let max = m.limits.max_memory;
    let mut c = Counts::new();
    let mut names = String::new();
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
        if !r.subrecords.iter().any(|s| {
            s.signature == *b"CTDA" && r.data(s).get(8..10) == Some(&FUNCTION.to_le_bytes()[..])
        }) {
            continue;
        }
        let sig = String::from_utf8_lossy(&r.signature).into_owned();
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        // Section 1: every list of the record.
        let lists = condition::lists(&r, &b)?;
        for l in &lists {
            for x in &l.conditions {
                if x.ctda.function != FUNCTION {
                    continue;
                }
                let t = &x.ctda;
                bump(&mut c, format!("1 conditions by host record type\t{sig}"));
                bump(
                    &mut c,
                    format!(
                        "1 by operator, flag bits and run-on\top {}\tflags 0x{:02x}\trun-on {}",
                        t.operator(),
                        t.flags(),
                        t.run_on
                    ),
                );
                bump(&mut c, format!("1 by comparison value\t{}", value(t)));
            }
        }
        if r.signature != *b"SCEN" {
            continue;
        }
        let s = Scene::read(&r, ctx, &b)?;
        // Sections 2 and 3: (part, phase or action index, condition).
        let mut held: Vec<(String, Option<usize>, Ctda)> = Vec::new();
        for (p, ph) in s.phases.iter().enumerate() {
            for x in &ph.start {
                held.push(("a phase's start conditions".into(), Some(p), x.ctda));
            }
            for x in &ph.completion {
                held.push(("a phase's completion conditions".into(), Some(p), x.ctda));
            }
        }
        for ac in &s.actions {
            for x in &ac.conditions {
                held.push(("an action's conditions".into(), None, x.ctda));
            }
        }
        for x in &s.conditions {
            held.push(("the scene's own conditions".into(), None, x.ctda));
        }
        for (part, phase, t) in held {
            if t.function != FUNCTION {
                continue;
            }
            bump(&mut c, format!("2 scene conditions by part\t{part}"));
            let first = condition::form(ctx, t.param1, &r.source.file, 0)
                .ok()
                .flatten();
            let names_scene = first
                .as_ref()
                .is_some_and(|f| store.signature(f) == Some(*b"SCEN"));
            let itself = first.as_ref() == Some(&k);
            bump(
                &mut c,
                format!(
                    "3 first parameter\t{}",
                    match (names_scene, itself) {
                        (true, true) => "the host scene itself",
                        (true, false) => "another scene",
                        _ => "not a scene",
                    }
                ),
            );
            let target = if itself {
                Some(s.clone())
            } else if names_scene {
                let f = first.clone().expect("a scene");
                store.raw_with_plugin(&f, &b)?.and_then(|(rr, pp)| {
                    Scene::read(
                        &rr,
                        Context {
                            order: store.order(),
                            plugin: pp,
                        },
                        &b,
                    )
                    .ok()
                })
            } else {
                None
            };
            let Some(target) = target else { continue };
            let found: Vec<_> = target
                .actions
                .iter()
                .filter(|x| x.index == t.param2)
                .collect();
            let [act] = found.as_slice() else {
                bump(
                    &mut c,
                    format!(
                        "3 second parameter\t{} actions of the named scene carry that index",
                        if found.is_empty() { "no" } else { "several" }
                    ),
                );
                continue;
            };
            bump(
                &mut c,
                format!(
                    "3 second parameter names one action\tof {}",
                    action_kind(act.kind)
                ),
            );
            if let (Some(p), true) = (phase, itself) {
                let p = p as u32;
                let rel = if act.start <= p && p <= act.end {
                    if act.start == p && act.end == p {
                        "the action starts and ends in the phase"
                    } else if act.end == p {
                        "the action ends in the phase, started earlier"
                    } else {
                        "the action spans the phase and ends later"
                    }
                } else if act.end < p {
                    "the action ended in an earlier phase"
                } else {
                    "the action starts in a later phase"
                };
                bump(
                    &mut c,
                    format!("3 the named action against the phase whose list holds the condition\t{part}\t{rel}"),
                );
            }
            let _ = writeln!(
                names,
                "## {} {} {part} {phase:?}: function 131 on action {} ({}) op {} value {}",
                record_key(&k),
                String::from_utf8_lossy(s.editor_id.as_deref().unwrap_or_default()),
                t.param2,
                action_kind(act.kind),
                t.operator(),
                value(&t)
            );
        }
    }
    let mut out = String::from(
        "# action_percent_census: kinds and counts only (spec/vm.md, Scene action progress). Sections: 1 conditions of function 131 in every winning record by host, operator, flags, run-on and value;\n# 2 in scene records by part; 3 what the parameters name: the scene, the action and its kind, the action against the phase whose list holds the condition.\n",
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
