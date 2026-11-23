#![forbid(unsafe_code)]
//! Character behaviour graphs, as far as the repository's readers go
//! (`spec/vm.md`, Character behaviour graphs; `spec/havok-behavior.md`):
//! `char_graph_census MANIFEST OUT.txt NAMES.txt [--actor PLUGIN:OBJECTID]
//! [--event NAME]...`.
//!
//! OUT.txt holds counts and kinds only (`spec/character-graph-census.txt`);
//! NAMES.txt names records, files and events and is never versioned. The
//! actor and the event names are the caller's (the events an actor's
//! registrations name in a run). Nothing runs:
//!
//! 1. every winning race (RACE): the texts of its MODL subrecords that end
//!    in `.hkx` after its NAM3 marker, and whether the chain those name
//!    resolves with `fo4_havok`'s readers: project, character, behaviour
//!    graph, its root state machine (states by generator class, event
//!    table);
//! 2. the caller's actor: its base (NAME), the base's race (RNAM), the
//!    race's files as in 1, and which of the caller's events the root
//!    graph's event table holds;
//! 3. every `.hkx` file: the behaviour graph files whose event table holds
//!    each of the caller's events, and the clips whose annotations name
//!    them, with the annotation times and the clip's duration
//!    (`AnimationHeader`);
//! 4. where those clips are named: a byte search of each clip's path below its animations folder
//!    in every behaviour, character and project file (a search of the
//!    stored bytes, not a reading of any field), and whether the actor's
//!    root graph file names the behaviour files found (the same search, with the path below its behaviours folder);
//! 5. what the readers do not read on the way from the actor to a clip.
use fo4_core::{Budget, Manifest};
use fo4_havok as hk;
use fo4_plugin::{typed::Store, FormKey, Record};
use fo4_scene::anim_graph::{folder_of, key_under};
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

/// The MODL texts ending in `.hkx` after a record's NAM3 marker.
fn race_projects(r: &Record) -> Vec<String> {
    let Some(at) = r.subrecords.iter().position(|s| s.signature == *b"NAM3") else {
        return Vec::new();
    };
    r.subrecords[at..]
        .iter()
        .filter(|s| s.signature == *b"MODL")
        .filter_map(|s| {
            let d = r.data(s);
            let t = String::from_utf8_lossy(d.strip_suffix(&[0]).unwrap_or(d)).into_owned();
            t.to_ascii_lowercase().ends_with(".hkx").then_some(t)
        })
        .collect()
}

/// A FormID word of a subrecord, resolved through the record's plugin.
fn word(store: &Store, r: &Record, plugin: usize, tag: &[u8; 4]) -> Option<FormKey> {
    let s = r.subrecords.iter().find(|s| s.signature == *tag)?;
    let d = r.data(s);
    let id = u32::from_le_bytes(d.get(..4)?.try_into().ok()?);
    store
        .order()
        .remap(plugin, id, &r.source.file, s.offset)
        .ok()
        .flatten()
}

/// The chain from a project file: the character file, the behaviour file
/// and its root machine, or why it stops.
struct Chain {
    character: String,
    behavior: String,
    machine: hk::Machine,
}
fn chain(vfs: &Vfs, project: &str, b: &Budget) -> Result<Chain, String> {
    let pk = key_under("meshes", project).ok_or("the project path is not valid")?;
    let pb = vfs
        .read(&pk, b)
        .map_err(|e| format!("project {}", e.code))?;
    let pc = hk::Container::read(&pb, &pk, 0, b).map_err(|e| e.code.to_string())?;
    let i = hk::only_object(&pc, hk::PROJECT_STRINGS).ok_or("not one project string object")?;
    let cs = hk::project_characters(&pc, i, &pk, 0, b).map_err(|e| e.code.to_string())?;
    let ch = match cs.as_slice() {
        [one] => String::from_utf8_lossy(one).into_owned(),
        [] => return Err("no character".into()),
        _ => return Err("several characters".into()),
    };
    let ck = key_under(folder_of(&pk), &ch).ok_or("the character path is not valid")?;
    let cb = vfs
        .read(&ck, b)
        .map_err(|e| format!("character {}", e.code))?;
    let cc = hk::Container::read(&cb, &ck, 0, b).map_err(|e| e.code.to_string())?;
    let i = hk::only_object(&cc, hk::CHARACTER_STRINGS).ok_or("not one character string object")?;
    let beh = hk::character_behavior(&cc, i, &ck, 0).map_err(|e| e.code.to_string())?;
    let bk = key_under(folder_of(&pk), &String::from_utf8_lossy(beh))
        .ok_or("the behaviour path is not valid")?;
    let bb = vfs
        .read(&bk, b)
        .map_err(|e| format!("behaviour {}", e.code))?;
    let bc = hk::Container::read(&bb, &bk, 0, b).map_err(|e| e.code.to_string())?;
    let machine = hk::Machine::read(&bc, &bk, 0, b).map_err(|e| e.code.to_string())?;
    Ok(Chain {
        character: ck,
        behavior: bk,
        machine,
    })
}

/// Whether `hay` holds `needle`, ignoring ASCII case and taking `/` and
/// `\` alike.
fn holds(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || hay.len() < needle.len() {
        return false;
    }
    let same = |a: u8, b: u8| {
        a.eq_ignore_ascii_case(&b) || (matches!(a, b'/' | b'\\') && matches!(b, b'/' | b'\\'))
    };
    hay.windows(needle.len())
        .any(|w| w.iter().zip(needle).all(|(a, b)| same(*a, *b)))
}

/// A file's path below its last `animations` or `behaviors` folder (the
/// whole key without one), without the extension, with a backslash
/// between folders as stored texts write it.
fn tail(key: &str) -> String {
    let lower = key.to_ascii_lowercase();
    let rest = ["/animations/", "/behaviors/"]
        .iter()
        .filter_map(|m| lower.rfind(m).map(|i| &key[i + m.len()..]))
        .next()
        .unwrap_or(key);
    let rest = rest.rsplit_once('.').map_or(rest, |(s, _)| s);
    rest.replace('/', "\\")
}

fn run() -> Result<(), E> {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let mut actor: Option<FormKey> = None;
    let mut events: Vec<String> = Vec::new();
    while let Some(i) = a.iter().position(|x| x == "--actor" || x == "--event") {
        let v = a
            .get(i + 1)
            .cloned()
            .ok_or("--actor PLUGIN:OBJECTID | --event NAME")?;
        if a[i] == "--actor" {
            let (p, id) = v.rsplit_once(':').ok_or("--actor PLUGIN:OBJECTID")?;
            actor = Some(FormKey {
                plugin: p.to_ascii_lowercase(),
                object_id: u32::from_str_radix(id, 16)? & 0x00ff_ffff,
            });
        } else {
            events.push(v);
        }
        a.drain(i..i + 2);
    }
    if a.len() != 3 {
        return Err(
            "char_graph_census MANIFEST OUT.txt NAMES.txt [--actor PLUGIN:OBJECTID] [--event NAME]..."
                .into(),
        );
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let max = m.limits.max_memory;
    let mut c = Counts::new();
    let mut names = String::new();
    *c.entry("0 caller's events".into()).or_default() += events.len() as u64;
    // 1: races.
    let races: Vec<FormKey> = store
        .winners()
        .filter(|w| !w.deleted && w.signature == *b"RACE")
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .collect();
    let mut chains: BTreeMap<String, Result<Chain, String>> = BTreeMap::new();
    let _ = writeln!(names, "# races: race, behaviour project texts after NAM3");
    for k in &races {
        let b = Budget::new(max);
        let Some(r) = store.raw(k, &b)? else { continue };
        bump(&mut c, "1 races read");
        let ps = race_projects(&r);
        bump(
            &mut c,
            format!(
                "1 races by MODL texts ending in .hkx after NAM3\t{}",
                ps.len()
            ),
        );
        let distinct: BTreeSet<String> = ps.iter().map(|p| p.to_ascii_lowercase()).collect();
        if ps.len() == 2 {
            bump(
                &mut c,
                format!(
                    "1 races with two such texts\t{}",
                    if distinct.len() == 1 {
                        "the same file twice"
                    } else {
                        "two files"
                    }
                ),
            );
        }
        let _ = writeln!(names, "{}\t{ps:?}", fo4_vm_key(k));
        for p in distinct {
            chains.entry(p).or_insert_with_key(|p| chain(&vfs, p, &b));
        }
    }
    *c.entry("1 distinct project files named by races".into())
        .or_default() += chains.len() as u64;
    let _ = writeln!(
        names,
        "# race projects: project, character, behaviour or why not"
    );
    for (p, x) in &chains {
        match x {
            Ok(ch) => {
                bump(
                    &mut c,
                    "1 race projects whose character and behaviour graph resolve",
                );
                *c.entry("1 race projects: root machine states".into())
                    .or_default() += ch.machine.states.len() as u64;
                *c.entry("1 race projects: root graph event table entries".into())
                    .or_default() += ch.machine.events.len() as u64;
                for s in &ch.machine.states {
                    bump(
                        &mut c,
                        format!(
                            "1 race projects: root machine states by generator class\t{}",
                            s.generator
                        ),
                    );
                }
                let _ = writeln!(
                    names,
                    "{p}\t{}\t{}\t{} states\t{} events",
                    ch.character,
                    ch.behavior,
                    ch.machine.states.len(),
                    ch.machine.events.len()
                );
            }
            Err(e) => {
                bump(&mut c, format!("1 race projects not resolving\t{e}"));
                let _ = writeln!(names, "{p}\t{e}");
            }
        }
    }
    // 2: the caller's actor.
    let mut actor_graph: Option<(String, String, BTreeSet<String>)> = None;
    if let Some(k) = &actor {
        let b = Budget::new(max);
        let sec = "2 the caller's actor";
        match store.raw_with_plugin(k, &b)? {
            None => bump(&mut c, format!("{sec}\tno winning record")),
            Some((r, plugin)) => {
                bump(
                    &mut c,
                    format!(
                        "{sec}\trecord type\t{}",
                        String::from_utf8_lossy(&r.signature)
                    ),
                );
                let base = word(&store, &r, plugin, b"NAME");
                let base_rec = base
                    .as_ref()
                    .and_then(|x| store.raw_with_plugin(x, &b).ok().flatten());
                let race = base_rec
                    .as_ref()
                    .and_then(|(br, bp)| word(&store, br, *bp, b"RNAM"));
                let race_rec = race.as_ref().and_then(|x| store.raw(x, &b).ok().flatten());
                bump(
                    &mut c,
                    format!(
                        "{sec}\tbase\t{}",
                        base_rec
                            .as_ref()
                            .map(|(br, _)| String::from_utf8_lossy(&br.signature).into_owned())
                            .unwrap_or_else(|| "none".into())
                    ),
                );
                if let Some((br, _)) = &base_rec {
                    bump(
                        &mut c,
                        format!(
                            "{sec}\tbase has a template (TPLT)\t{}",
                            br.subrecords.iter().any(|s| s.signature == *b"TPLT")
                        ),
                    );
                }
                let ps = race_rec.as_ref().map(race_projects).unwrap_or_default();
                bump(
                    &mut c,
                    format!(
                        "{sec}\trace\t{}, {} MODL texts ending in .hkx after NAM3",
                        if race_rec.is_some() { "read" } else { "none" },
                        ps.len()
                    ),
                );
                let distinct: BTreeSet<String> =
                    ps.iter().map(|p| p.to_ascii_lowercase()).collect();
                bump(
                    &mut c,
                    format!(
                        "{sec}\tdistinct project files of its race\t{}",
                        distinct.len()
                    ),
                );
                let _ = writeln!(
                    names,
                    "# actor {}: base {:?}, race {:?}, projects {ps:?}",
                    fo4_vm_key(k),
                    base.as_ref().map(fo4_vm_key),
                    race.as_ref().map(fo4_vm_key)
                );
                for p in &distinct {
                    match chains.get(p) {
                        Some(Ok(ch)) => {
                            bump(&mut c, format!("{sec}\tits race's chain\tresolves"));
                            let ev: BTreeSet<String> = ch
                                .machine
                                .events
                                .iter()
                                .map(|e| e.to_ascii_lowercase())
                                .collect();
                            let held = events
                                .iter()
                                .filter(|e| ev.contains(&e.to_ascii_lowercase()))
                                .count();
                            *c.entry(format!(
                                "{sec}\tcaller's events the root graph's event table holds"
                            ))
                            .or_default() += held as u64;
                            *c.entry(format!("{sec}\troot machine states")).or_default() +=
                                ch.machine.states.len() as u64;
                            for s in &ch.machine.states {
                                bump(
                                    &mut c,
                                    format!(
                                        "{sec}\troot machine states by generator class\t{}",
                                        s.generator
                                    ),
                                );
                            }
                            actor_graph = Some((ch.character.clone(), ch.behavior.clone(), ev));
                        }
                        Some(Err(e)) => bump(&mut c, format!("{sec}\tits race's chain\t{e}")),
                        None => bump(&mut c, format!("{sec}\tits race's chain\tnot read")),
                    }
                }
            }
        }
    }
    // 3: every .hkx: graphs declaring the events, clips annotating them.
    let wanted: BTreeMap<String, String> = events
        .iter()
        .map(|e| (e.to_ascii_lowercase(), e.clone()))
        .collect();
    let mut graphs_with: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    // clip key -> (duration, event -> times)
    let mut clips: BTreeMap<String, (f32, BTreeMap<String, Vec<f32>>)> = BTreeMap::new();
    let mut behaviour_files: Vec<String> = Vec::new();
    let all = keys(&vfs, b"", b".hkx");
    for k in &all {
        let b = Budget::new(max);
        let Ok(bytes) = vfs.read(k, &b) else {
            continue;
        };
        let Ok(hc) = hk::Container::read(&bytes, k, 0, &b) else {
            continue;
        };
        let graph = hc.objects.iter().any(|o| o.class == hk::BEHAVIOR_GRAPH);
        let refs = graph
            || hc
                .objects
                .iter()
                .any(|o| o.class == hk::CHARACTER_STRINGS || o.class == hk::PROJECT_STRINGS);
        if refs {
            behaviour_files.push(k.clone());
        }
        if graph {
            if let Ok(mach) = hk::Machine::read(&hc, k, 0, &b) {
                for e in &mach.events {
                    if let Some(w) = wanted.get(&e.to_ascii_lowercase()) {
                        graphs_with.entry(w.clone()).or_default().insert(k.clone());
                    }
                }
            }
        }
        if wanted.is_empty() {
            continue;
        }
        for (i, o) in hc.objects.iter().enumerate() {
            if o.class != hk::SPLINE && o.class != hk::LOSSLESS {
                continue;
            }
            let Ok(tracks) = hk::annotation_tracks(&hc, i, k, 0, &b) else {
                continue;
            };
            let mut found: BTreeMap<String, Vec<f32>> = BTreeMap::new();
            for t in &tracks {
                for x in &t.annotations {
                    let s = String::from_utf8_lossy(x.text).to_ascii_lowercase();
                    if let Some(w) = wanted.get(&s) {
                        found.entry(w.clone()).or_default().push(x.time);
                    }
                }
            }
            if found.is_empty() {
                continue;
            }
            let d = hk::AnimationHeader::read(&hc, i, k, 0)
                .map(|h| h.duration)
                .unwrap_or(f32::NAN);
            clips.insert(format!("{k}#{i}"), (d, found));
        }
    }
    let sec = "3 the caller's events";
    for e in &events {
        let g = graphs_with.get(e).map_or(0, |s| s.len());
        bump(
            &mut c,
            format!("{sec}\tbehaviour graph files whose event table holds it\t{g}"),
        );
        let n = clips.values().filter(|(_, f)| f.contains_key(e)).count();
        bump(
            &mut c,
            format!("{sec}\tclips whose annotations name it\t{n}"),
        );
    }
    *c.entry(format!("{sec}\tclips naming at least one of them"))
        .or_default() += clips.len() as u64;
    let every = clips
        .values()
        .filter(|(_, f)| events.iter().all(|e| f.contains_key(e)))
        .count();
    *c.entry(format!("{sec}\tclips naming every one of them"))
        .or_default() += every as u64;
    let graph_files: BTreeSet<&String> = graphs_with.values().flatten().collect();
    *c.entry(format!(
        "{sec}\tbehaviour graph files holding at least one in their event table"
    ))
    .or_default() += graph_files.len() as u64;
    let _ = writeln!(
        names,
        "# graphs holding the events: event, files\n{:#?}",
        graphs_with
    );
    let _ = writeln!(names, "# clips: clip#object, duration, event times");
    for (k, (d, f)) in &clips {
        let _ = writeln!(names, "{k}\t{d}\t{f:?}");
        // Times in order of the caller's events, as the clip stores them.
        let mut last = f32::NEG_INFINITY;
        let mut ordered = true;
        for e in &events {
            if let Some(t) = f.get(e).and_then(|v| v.first()) {
                if *t < last {
                    ordered = false;
                }
                last = *t;
            }
        }
        bump(
            &mut c,
            format!(
                "{sec}\tclips whose first annotation times follow the caller's order of the events\t{ordered}"
            ),
        );
        let within = f.values().flatten().all(|t| *t >= 0.0 && *t <= *d);
        bump(
            &mut c,
            format!("{sec}\tclips whose annotation times lie within the clip's duration\t{within}"),
        );
        let repeated = f.values().any(|v| v.len() > 1);
        bump(
            &mut c,
            format!("{sec}\tclips naming one of the events more than once\t{repeated}"),
        );
    }
    // 4: where the clips are named (a byte search of the path).
    let sec = "4 where the clips are named (a byte search of the path below the animations folder)";
    let tails: BTreeMap<String, String> = clips
        .keys()
        .map(|k| {
            let file = k.rsplit_once('#').map_or(k.as_str(), |(f, _)| f);
            (file.to_string(), tail(file))
        })
        .collect();
    // Each file is counted by the longest part of the clip's path it holds,
    // in folders from the end (1: the file name alone); it names the clip
    // when it holds two folders or more.
    let mut naming: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for k in &behaviour_files {
        let b = Budget::new(max);
        let Ok(bytes) = vfs.read(k, &b) else { continue };
        for (file, s) in &tails {
            let parts: Vec<&str> = s.split('\\').collect();
            let longest = (0..parts.len())
                .find(|i| holds(&bytes, parts[*i..].join("\\").as_bytes()))
                .map(|i| parts.len() - i)
                .unwrap_or(0);
            bump(
                &mut c,
                format!(
                    "{sec}\tbehaviour, character and project files by the longest part of a clip's path they hold, in path parts of {}\t{longest}",
                    parts.len()
                ),
            );
            if longest >= 2 {
                naming.entry(file.clone()).or_default().insert(k.clone());
            }
        }
    }
    for file in tails.keys() {
        let n = naming.get(file).map_or(0, |s| s.len());
        bump(
            &mut c,
            format!(
                "{sec}\tclip files by the behaviour, character and project files naming them\t{n}"
            ),
        );
    }
    let referencing: BTreeSet<String> = naming.values().flatten().cloned().collect();
    *c.entry(format!(
        "{sec}\tbehaviour, character and project files naming a clip"
    ))
    .or_default() += referencing.len() as u64;
    let _ = writeln!(names, "# files naming each clip\n{naming:#?}");
    if let Some((ck, bk, _)) = &actor_graph {
        let b = Budget::new(max);
        for (what, k) in [("character file", ck), ("root graph file", bk)] {
            let Ok(bytes) = vfs.read(k, &b) else { continue };
            for r in &referencing {
                if r == k {
                    bump(
                        &mut c,
                        format!("{sec}\tthe actor's {what} is itself a file naming a clip"),
                    );
                    continue;
                }
                let s = tail(r);
                bump(
                    &mut c,
                    format!(
                        "{sec}\tthe actor's {what} names a file naming a clip (its path below the behaviours folder)\t{}",
                        holds(&bytes, s.as_bytes())
                    ),
                );
            }
        }
    }
    // 4 (records): winning records whose subrecord texts hold two path
    // parts or more of a clip's path, by record type and tag.
    let needles: Vec<String> = tails
        .values()
        .filter_map(|t| {
            let parts: Vec<&str> = t.split('\\').collect();
            (parts.len() >= 2).then(|| parts[parts.len() - 2..].join("\\"))
        })
        .collect();
    let mut hits = 0u64;
    let _ = writeln!(
        names,
        "# records whose texts hold a clip's path (two parts or more)"
    );
    if !needles.is_empty() {
        let winners: Vec<FormKey> = store
            .winners()
            .filter(|w| !w.deleted)
            .map(|w| store.overrides().form_key(w.key, store.order()))
            .collect();
        for k in &winners {
            let b = Budget::new(max);
            let Ok(Some(r)) = store.raw(k, &b) else {
                continue;
            };
            for sub in &r.subrecords {
                let d = r.data(sub);
                if needles.iter().any(|n| holds(d, n.as_bytes())) {
                    hits += 1;
                    bump(
                        &mut c,
                        format!(
                            "{sec}	winning records' subrecords holding a clip's last two path parts, by type and tag\t{} {}",
                            String::from_utf8_lossy(&r.signature),
                            String::from_utf8_lossy(&sub.signature)
                        ),
                    );
                    let _ = writeln!(
                        names,
                        "{}\t{}\t{}",
                        fo4_vm_key(k),
                        String::from_utf8_lossy(&sub.signature),
                        String::from_utf8_lossy(d)
                    );
                }
            }
        }
    }
    *c.entry(format!(
        "{sec}\twinning records' subrecords holding a clip's last two path parts"
    ))
    .or_default() += hits;
    // 5: what the readers do not read (fixed text, from havok-behavior.md).
    for x in [
        "hkbClipGenerator fields (the clip a state plays, its mode, its start time)",
        "hkbBehaviorReferenceGenerator fields (the behaviour file a state runs)",
        "nested state machines below the root (only the root machine is read)",
        "transition conditions and the graph's variables",
        "the character file's animation list",
        "how a furniture's keywords or an actor's keywords select a graph state",
    ] {
        c.insert(format!("5 not read by the repository's readers\t{x}"), 1);
    }
    let mut out = String::from(
        "# char_graph_census: kinds and counts only (spec/vm.md, Character behaviour graphs; spec/havok-behavior.md). Sections: 0 the caller's input; 1 races and the behaviour project files they name;\n# 2 the caller's actor (base, race, chain, root graph); 3 graph files and clips holding the caller's events; 4 where those clips are named (a byte search of their paths below the animations folder); 5 what the readers do not read on the way from an actor to a clip.\n",
    );
    for (k, v) in &c {
        let _ = writeln!(out, "{v}\t{k}");
    }
    std::fs::write(&a[1], out)?;
    std::fs::write(&a[2], names)?;
    Ok(())
}

/// A record key as the effect log writes it.
fn fo4_vm_key(k: &FormKey) -> String {
    format!("{}:{:08X}", k.plugin.to_ascii_lowercase(), k.object_id)
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
