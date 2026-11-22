#![forbid(unsafe_code)]
//! Animation events, the data the animation step reads (`spec/vm.md`,
//! Animation events; `spec/havok-behavior.md`):
//! `anim_event_census MANIFEST OUT.txt NAMES.txt`.
//!
//! OUT.txt holds counts and kinds only (`spec/animation-event-census.txt`);
//! NAMES.txt names scripts, events and files and is never versioned.
//! Nothing runs:
//!
//! 1. every winning compiled script: the calls of
//!    `RegisterForAnimationEvent` and `UnregisterForAnimationEvent`, by how
//!    the sender argument is obtained, its declared type, and whether the
//!    event name is a constant text, a script variable with a stored text,
//!    or something else; the scripts that declare `OnAnimationEvent`;
//! 2. where the registered names occur (exact text, or only ignoring
//!    case): annotation texts of every clip, event-name tables of every
//!    behaviour graph, text keys of every NIF controller sequence, and
//!    text subrecords of IDLE, SCEN and PACK records, by tag;
//! 3. annotation tracks of every clip (`fo4_havok::annotation_tracks`):
//!    clips read or refused, tracks, annotations;
//! 4. behaviour graph files (`fo4_havok::Machine`): read or refused,
//!    the measured offsets (start state among the state ids, transition
//!    events within the event table, targets among the state ids), states
//!    by generator class, machines whose every state plays a Gamebryo
//!    sequence;
//! 5. the file chain from a NIF's behaviour graph extra data to the graph
//!    (`fo4_scene::anim_graph`): NIFs naming a graph file, project files
//!    that resolve, characters and behaviours that resolve, sequence
//!    generators whose sequence the NIF holds.
use fo4_core::{Budget, Game, Manifest};
use fo4_havok as hk;
use fo4_pex::{Pex, Value};
use fo4_plugin::typed::Store;
use fo4_scene::anim_graph::{folder_of, key_under, nif_animation};
use fo4_vfs::Vfs;
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

/// Winning archive keys with a prefix and a suffix, sorted.
fn keys(vfs: &Vfs, prefix: &[u8], suffix: &[u8]) -> Vec<String> {
    let mut out: Vec<String> = vfs
        .candidates
        .iter()
        .filter_map(|c| c.key.as_ref())
        .filter(|k| k.starts_with(prefix) && k.ends_with(suffix))
        .map(|k| String::from_utf8_lossy(k).into_owned())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Opcodes of the calls (`spec/pex.md`).
const CALL_METHOD: u8 = 23;
const CALL_STATIC: u8 = 25;
const ASSIGN: u8 = 14;

/// How a local or temporary was last written before instruction `at`
/// (one level of copy followed).
fn source_of(p: &Pex<'_>, f: &fo4_pex::Function, at: usize, name: &[u8], depth: u8) -> String {
    for ins in f.code[..at].iter().rev() {
        let dest = match ins.op {
            CALL_METHOD | CALL_STATIC => ins.args.get(2),
            ASSIGN => ins.args.first(),
            _ => None,
        };
        let Some(Value::Ident(d)) = dest else {
            continue;
        };
        if p.str(*d) != name {
            continue;
        }
        return match ins.op {
            CALL_STATIC => match (ins.args.first(), ins.args.get(1)) {
                (Some(Value::Ident(o)), Some(Value::Ident(n))) => format!(
                    "the result of {}.{}",
                    String::from_utf8_lossy(p.str(*o)).to_ascii_lowercase(),
                    String::from_utf8_lossy(p.str(*n)).to_ascii_lowercase()
                ),
                _ => "the result of a global call".into(),
            },
            CALL_METHOD => match ins.args.first() {
                Some(Value::Ident(n)) => format!(
                    "the result of a method {}",
                    String::from_utf8_lossy(p.str(*n)).to_ascii_lowercase()
                ),
                _ => "the result of a method".into(),
            },
            _ => match ins.args.get(1) {
                Some(Value::Ident(s)) if depth == 0 => {
                    let s = p.str(*s);
                    let pos = f
                        .code
                        .iter()
                        .position(|x| x.offset == ins.offset)
                        .unwrap_or(0);
                    kind_of_ident(p, f, pos, s, 1)
                }
                _ => "a copy of a value".into(),
            },
        };
    }
    "a local never written before the call".into()
}

/// What an identifier argument names, at instruction `at`.
fn kind_of_ident(p: &Pex<'_>, f: &fo4_pex::Function, at: usize, s: &[u8], depth: u8) -> String {
    if s.eq_ignore_ascii_case(b"self") {
        return "self".into();
    }
    if f.params.iter().any(|(n, _)| p.str(*n) == s) {
        return "a parameter".into();
    }
    if f.locals.iter().any(|(n, _)| p.str(*n) == s) {
        return source_of(p, f, at, s, depth);
    }
    let prop = p.objects.iter().any(|o| {
        o.properties
            .iter()
            .any(|x| x.variable.is_some_and(|v| p.str(v) == s))
    });
    if prop {
        return "a property's variable".into();
    }
    "a script variable".into()
}

/// The declared type of an identifier, lower case.
fn type_of(p: &Pex<'_>, f: &fo4_pex::Function, s: &[u8]) -> String {
    let t = f
        .params
        .iter()
        .chain(f.locals.iter())
        .find(|(n, _)| p.str(*n) == s)
        .map(|(_, t)| *t)
        .or_else(|| {
            p.objects
                .iter()
                .flat_map(|o| o.variables.iter())
                .find(|v| p.str(v.name) == s)
                .map(|v| v.type_name)
        });
    match t {
        Some(t) => String::from_utf8_lossy(p.str(t)).to_ascii_lowercase(),
        None if s.eq_ignore_ascii_case(b"self") => "self".into(),
        None => "not declared".into(),
    }
}

/// A script variable's stored text, when it has one.
fn variable_text(p: &Pex<'_>, s: &[u8]) -> Option<String> {
    p.objects
        .iter()
        .flat_map(|o| o.variables.iter())
        .find(|v| p.str(v.name) == s)
        .and_then(|v| match v.value {
            Value::Str(t) => Some(String::from_utf8_lossy(p.str(t)).into_owned()),
            _ => None,
        })
}

fn run() -> Result<(), E> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 3 {
        return Err("anim_event_census MANIFEST OUT.txt NAMES.txt".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let mut c = Counts::new();
    let mut names = String::new();
    // 1: registrations in the compiled scripts.
    let mut events: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut handlers = BTreeSet::new();
    let mut registering = BTreeSet::new();
    let _ = writeln!(
        names,
        "# registrations: script, call, sender source and type, event"
    );
    for k in keys(&vfs, b"scripts/", b".pex") {
        let b = Budget::new(m.limits.max_memory);
        let Ok(bytes) = vfs.read(&k, &b) else {
            bump(&mut c, "1 scripts\tnot read");
            continue;
        };
        let Ok(p) = Pex::read(&bytes, &k, &b) else {
            bump(&mut c, "1 scripts\tnot read");
            continue;
        };
        bump(&mut c, "1 scripts\tread");
        for o in &p.objects {
            for st in &o.states {
                for nf in &st.functions {
                    if p.str(nf.name).eq_ignore_ascii_case(b"onanimationevent") {
                        handlers.insert(k.clone());
                    }
                    let f = &nf.function;
                    for (at, ins) in f.code.iter().enumerate() {
                        if ins.op != CALL_METHOD {
                            continue;
                        }
                        let Some(Value::Ident(n)) = ins.args.first() else {
                            continue;
                        };
                        let n = String::from_utf8_lossy(p.str(*n)).to_ascii_lowercase();
                        if n != "registerforanimationevent" && n != "unregisterforanimationevent" {
                            continue;
                        }
                        let (sender, stype) = match ins.extra.first() {
                            Some(Value::Ident(s)) => {
                                let s = p.str(*s);
                                (kind_of_ident(&p, f, at, s, 0), type_of(&p, f, s))
                            }
                            Some(Value::None) => ("none".into(), "none".into()),
                            _ => ("a constant".into(), "a constant".into()),
                        };
                        let (ename, ekind) = match ins.extra.get(1) {
                            Some(Value::Str(t)) => (
                                Some(String::from_utf8_lossy(p.str(*t)).into_owned()),
                                "a constant text",
                            ),
                            Some(Value::Ident(s)) => match variable_text(&p, p.str(*s)) {
                                Some(t) => (Some(t), "a script variable with a stored text"),
                                None => (None, "a variable without a stored text"),
                            },
                            _ => (None, "another value"),
                        };
                        let receiver = match ins.args.get(1) {
                            Some(Value::Ident(r)) if p.str(*r).eq_ignore_ascii_case(b"self") => {
                                "on self"
                            }
                            _ => "on another script object",
                        };
                        bump(&mut c, format!("1 calls\t{n}\t{receiver}"));
                        bump(&mut c, format!("1 calls\t{n}\tsender\t{sender}"));
                        bump(&mut c, format!("1 calls\t{n}\tsender type\t{stype}"));
                        bump(&mut c, format!("1 calls\t{n}\tevent name\t{ekind}"));
                        if n == "registerforanimationevent" {
                            registering.insert(k.clone());
                            if let Some(e) = &ename {
                                events.entry(e.clone()).or_default().insert(k.clone());
                            }
                        }
                        let _ = writeln!(
                            names,
                            "{k}\t{n}\t{sender} ({stype})\t{}",
                            ename.as_deref().unwrap_or("?")
                        );
                    }
                }
            }
        }
    }
    *c.entry("1 scripts\tdeclaring OnAnimationEvent".into())
        .or_default() += handlers.len() as u64;
    *c.entry("1 scripts\tcalling RegisterForAnimationEvent".into())
        .or_default() += registering.len() as u64;
    *c.entry("1 scripts\tboth".into()).or_default() +=
        handlers.intersection(&registering).count() as u64;
    *c.entry("1 event names\tdistinct registered names (constant or stored)".into())
        .or_default() += events.len() as u64;
    // Where the names occur: (class) -> names found exactly / ignoring case.
    let lower: BTreeMap<String, String> = events
        .keys()
        .map(|e| (e.to_ascii_lowercase(), e.clone()))
        .collect();
    let mut found: BTreeMap<String, BTreeSet<(String, bool)>> = BTreeMap::new();
    let seen =
        |class: &str, text: &[u8], found: &mut BTreeMap<String, BTreeSet<(String, bool)>>| {
            let t = String::from_utf8_lossy(text);
            if let Some(e) = lower.get(&t.to_ascii_lowercase()) {
                found
                    .entry(e.clone())
                    .or_default()
                    .insert((class.to_string(), *e == t));
            }
        };
    // 3: annotation tracks; 4: behaviour graphs; both from the .hkx files.
    let mut graph_chain: Counts = Counts::new();
    for k in keys(&vfs, b"", b".hkx") {
        let b = Budget::new(m.limits.max_memory);
        let Ok(bytes) = vfs.read(&k, &b) else {
            bump(&mut c, "3 hkx files\tnot read");
            continue;
        };
        let Ok(hc) = hk::Container::read(&bytes, &k, 0, &b) else {
            bump(&mut c, "3 hkx files\tcontainer refused");
            continue;
        };
        for (i, o) in hc.objects.iter().enumerate() {
            if o.class != hk::SPLINE && o.class != hk::LOSSLESS {
                continue;
            }
            match hk::annotation_tracks(&hc, i, &k, 0, &b) {
                Ok(tracks) => {
                    bump(&mut c, "3 clips\tannotation tracks read");
                    *c.entry("3 clips\ttracks".into()).or_default() += tracks.len() as u64;
                    let n: usize = tracks.iter().map(|t| t.annotations.len()).sum();
                    *c.entry("3 clips\tannotations".into()).or_default() += n as u64;
                    if n > 0 {
                        bump(&mut c, "3 clips\twith at least one annotation");
                    }
                    for t in &tracks {
                        for x in &t.annotations {
                            seen("an annotation text of a clip", x.text, &mut found);
                        }
                    }
                }
                Err(e) => bump(&mut c, format!("3 clips\trefused\t{}", e.code)),
            }
        }
        if hc.objects.iter().any(|o| o.class == hk::PROJECT_STRINGS) {
            bump(&mut graph_chain, "5 chain\tproject files");
            let chain = hk::only_object(&hc, hk::PROJECT_STRINGS)
                .ok_or("not one project string object".to_string())
                .and_then(|i| hk::project_characters(&hc, i, &k, 0, &b).map_err(|e| e.code))
                .and_then(|cs| match cs.as_slice() {
                    [one] => Ok(String::from_utf8_lossy(one).into_owned()),
                    [] => Err("no character".into()),
                    _ => Err("several characters".into()),
                })
                .and_then(|ch| key_under(folder_of(&k), &ch).ok_or("not a valid path".into()))
                .and_then(|ck| {
                    let cb = vfs
                        .read(&ck, &b)
                        .map_err(|e| format!("character {}", e.code))?;
                    let cc = hk::Container::read(&cb, &ck, 0, &b).map_err(|e| e.code)?;
                    let i = hk::only_object(&cc, hk::CHARACTER_STRINGS)
                        .ok_or("not one character string object")?;
                    let beh = hk::character_behavior(&cc, i, &ck, 0).map_err(|e| e.code)?;
                    let bk = key_under(folder_of(&k), &String::from_utf8_lossy(beh))
                        .ok_or("the behaviour path is not valid")?;
                    let bb = vfs
                        .read(&bk, &b)
                        .map_err(|e| format!("behaviour {}", e.code))?;
                    let bc = hk::Container::read(&bb, &bk, 0, &b).map_err(|e| e.code)?;
                    bc.objects
                        .iter()
                        .any(|o| o.class == hk::BEHAVIOR_GRAPH)
                        .then_some(())
                        .ok_or_else(|| "the behaviour file holds no graph".to_string())
                });
            match chain {
                Ok(()) => bump(
                    &mut graph_chain,
                    "5 chain\tproject files whose character and behaviour graph resolve",
                ),
                Err(e) => bump(
                    &mut graph_chain,
                    format!("5 chain\tproject files not resolving\t{e}"),
                ),
            }
        }
        if !hc.objects.iter().any(|o| o.class == hk::BEHAVIOR_GRAPH) {
            continue;
        }
        bump(&mut c, "4 behaviour graph files");
        match hk::Machine::read(&hc, &k, 0, &b) {
            Ok(mach) => {
                bump(&mut c, "4 root state machines read");
                for e in &mach.events {
                    seen(
                        "an event name of a behaviour graph",
                        e.as_bytes(),
                        &mut found,
                    );
                }
                let ids: BTreeSet<i32> = mach.states.iter().map(|s| s.id).collect();
                bump(
                    &mut c,
                    format!(
                        "4 root machines\tstart state (+168) among the state ids\t{}",
                        ids.contains(&mach.start)
                    ),
                );
                let mut all_seq = !mach.states.is_empty();
                for s in &mach.states {
                    bump(
                        &mut c,
                        format!(
                            "4 root machines\tstates by generator class\t{}",
                            s.generator
                        ),
                    );
                    if s.sequence.is_none() {
                        all_seq = false;
                    }
                    for t in &s.transitions {
                        bump(
                            &mut c,
                            format!(
                                "4 root machines\ttransition event (+48) within the event table\t{}",
                                t.event >= 0 && (t.event as usize) < mach.events.len()
                            ),
                        );
                        bump(
                            &mut c,
                            format!(
                                "4 root machines\ttransition target (+52) among the state ids\t{}",
                                ids.contains(&t.to_state)
                            ),
                        );
                        bump(
                            &mut c,
                            format!(
                                "4 root machines\ttransition with a condition (+40)\t{}",
                                t.condition.is_some()
                            ),
                        );
                    }
                }
                for t in &mach.wildcards {
                    bump(
                        &mut c,
                        format!(
                            "4 root machines\twildcard transition event within the event table\t{}",
                            t.event >= 0 && (t.event as usize) < mach.events.len()
                        ),
                    );
                }
                if all_seq {
                    bump(
                        &mut c,
                        "4 root machines\tevery state plays a Gamebryo sequence",
                    );
                }
            }
            Err(e) => bump(
                &mut c,
                format!("4 root state machines not read\t{}", e.code),
            ),
        }
    }
    // 5 and 2: NIF text keys and the chain from a NIF.
    for k in keys(&vfs, b"meshes/", b".nif") {
        let b = Budget::new(m.limits.max_memory);
        let Ok(bytes) = vfs.read(&k, &b) else {
            continue;
        };
        let has = |pat: &[u8]| bytes.windows(pat.len()).any(|w| w == pat);
        if !has(b"NiTextKeyExtraData") && !has(b"BSBehaviorGraphExtraData") {
            continue;
        }
        let Ok(n) = fo4_nif::Nif::read(&bytes, &k, Game::Fallout4, &m.limits, &b) else {
            bump(
                &mut graph_chain,
                "5 chain\tNIFs with text keys or a graph not read",
            );
            continue;
        };
        let anim = nif_animation(&n);
        for s in &anim.sequences {
            bump(&mut c, "2 NIF controller sequences");
            for (_, t) in &s.keys {
                seen(
                    "a text key of a NIF controller sequence",
                    t.as_bytes(),
                    &mut found,
                );
            }
        }
        let Some(g) = anim.graph_file else { continue };
        bump(
            &mut graph_chain,
            "5 chain\tNIFs naming a behaviour graph file",
        );
        let resolves = key_under("meshes", &g)
            .and_then(|pk| vfs.read(&pk, &b).ok())
            .and_then(|pb| {
                hk::Container::read(&pb, &g, 0, &b)
                    .ok()
                    .map(|pc| pc.objects.iter().any(|o| o.class == "hkbProjectData"))
            });
        bump(
            &mut graph_chain,
            format!(
                "5 chain\tNIFs whose graph file under meshes is a project file\t{}",
                resolves == Some(true)
            ),
        );
    }
    // 2: text subrecords of IDLE, SCEN and PACK records.
    for sig in [*b"IDLE", *b"SCEN", *b"PACK"] {
        for w in store.winners().filter(|w| !w.deleted && w.signature == sig) {
            let k = store.overrides().form_key(w.key, store.order());
            let Ok(Some(r)) = store.raw(&k, &Budget::new(m.limits.max_memory)) else {
                continue;
            };
            for s in &r.subrecords {
                let d = r.data(s);
                let t = d.strip_suffix(&[0]).unwrap_or(d);
                if t.is_empty() || !t.iter().all(|x| x.is_ascii_graphic() || *x == b' ') {
                    continue;
                }
                let class = format!(
                    "a {} subrecord {}",
                    String::from_utf8_lossy(&sig),
                    String::from_utf8_lossy(&s.signature)
                );
                seen(&class, t, &mut found);
            }
        }
    }
    let _ = writeln!(
        names,
        "# registered event names and where they occur (class, exact text)"
    );
    let mut classes: Counts = Counts::new();
    let mut nowhere = 0u64;
    for (e, scripts) in &events {
        let f = found.get(e).cloned().unwrap_or_default();
        if f.is_empty() {
            nowhere += 1;
        }
        let mut per: BTreeMap<String, bool> = BTreeMap::new();
        for (class, exact) in &f {
            let x = per.entry(class.clone()).or_insert(false);
            *x |= *exact;
        }
        for (class, exact) in &per {
            bump(
                &mut classes,
                format!(
                    "2 registered names found\t{class}\t{}",
                    if *exact {
                        "exact text"
                    } else {
                        "only ignoring case"
                    }
                ),
            );
        }
        let _ = writeln!(
            names,
            "{e}\t{} script(s)\t{:?}",
            scripts.len(),
            per.iter().collect::<Vec<_>>()
        );
    }
    *classes
        .entry("2 registered names found\tin none of these places".into())
        .or_default() += nowhere;
    c.extend(classes);
    c.extend(graph_chain);
    let mut out = String::from(
        "# anim_event_census: kinds and counts only (spec/vm.md, Animation events; spec/havok-behavior.md). Sections: 1 registration calls in the compiled scripts;\n# 2 where the registered names occur; 3 annotation tracks of the clips; 4 behaviour graph files and their root state machines; 5 the file chain from a NIF to its graph.\n",
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
