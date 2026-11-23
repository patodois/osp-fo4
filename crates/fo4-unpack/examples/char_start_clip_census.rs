#![forbid(unsafe_code)]
//! Census of the character behaviour graphs a run timeline's actors use,
//! as far as `fo4_scene::char_graph` reads them (`spec/character-graph.md`,
//! `spec/character-start-clip-census.txt`):
//! `char_start_clip_census MANIFEST TIMELINE OUT.txt NAMES.txt`.
//!
//! OUT.txt holds counts and kinds only; NAMES.txt names records, files
//! and clips and is never versioned. Nothing runs. Sections:
//!
//! 0. the actors the timeline places and how they resolve (race, section);
//! 1. the graph files of their races: the root graph of each project, the
//!    files behaviour references name, the files the races' subgraph
//!    entries name; objects by class;
//! 2. state machines, 3. clip generators (with the clip file each reading
//!    of a clip text finds), 4. behaviour references, 5. blenders,
//!    6. manual selectors, 7. variables, of those files;
//! 8. the path from each file's root followed by stored start states,
//!    stored indices and the first of the highest stored weights alone,
//!    to where it ends, by length;
//! 9. the races' subgraph entries and the entries each actor's keywords fit;
//! 10. the walk of `fo4_scene::char_graph` for each actor with every
//!     switch on: the clip reached or the refusal, the path's length, the
//!     clip's mode, length and fit to the skeleton;
//! 11. the layout checks over every behaviour graph file of the
//!     installation (the measurement behind the offsets read);
//! 12. sitting: the timeline's event kinds (none says who sits on what)
//!     and, for every furniture record, the subgraph entries of flag word
//!     2 its keywords fit.
use fo4_core::{Budget, Manifest, SourceSpan};
use fo4_havok as hk;
use fo4_plugin::{typed::Store, FormKey};
use fo4_scene::{
    actor,
    anim_graph::{folder_of, key_under},
    char_graph::{self, Switches},
    timeline::{self as tl, Timeline, What},
    Loader, RuleLevel,
};
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
fn add(m: &mut Counts, k: impl Into<String>, n: u64) {
    *m.entry(k.into()).or_default() += n;
}
fn key_text(k: &FormKey) -> String {
    format!("{}:{:08X}", k.plugin.to_ascii_lowercase(), k.object_id)
}
fn exists(vfs: &Vfs, k: &str) -> bool {
    vfs.chain(k).is_ok_and(|c| !c.is_empty())
}

/// How a graph file entered the census.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Via {
    Root,
    Reference,
    Subgraph,
}
impl Via {
    fn text(self) -> &'static str {
        match self {
            Via::Root => "a race project's root graph",
            Via::Reference => "named by a behaviour reference",
            Via::Subgraph => "named by a subgraph entry",
        }
    }
}

/// Where the census walk of one file ends.
fn file_walk(c: &hk::Container<'_>, file: &str, b: &Budget) -> (String, usize) {
    let Some(g) = hk::only_object(c, hk::BEHAVIOR_GRAPH) else {
        return ("no graph".into(), 0);
    };
    let Ok(Some(mut at)) = hk::BehaviorGraph::read(c, g, file, 0).map(|x| x.root) else {
        return ("no root".into(), 0);
    };
    let mut n = 0;
    loop {
        n += 1;
        if n > char_graph::MAX_STEPS {
            return ("step limit".into(), n);
        }
        let Ok(node) = hk::Node::read(c, at, file, 0, b) else {
            return ("unreadable node".into(), n);
        };
        let next = match node {
            hk::Node::Machine(m) => match hk::machine_state(c, &m, m.start, file, 0) {
                Ok(Some(s)) => s.generator,
                _ => return ("a start state that is none of its machine's".into(), n),
            },
            hk::Node::Wrapper { generator } | hk::Node::Offset { generator, .. } => generator,
            hk::Node::Blender { children, .. } => {
                let mut best: Option<&hk::BlendChild> = None;
                for ch in &children {
                    if best.is_none_or(|x| ch.weight > x.weight) {
                        best = Some(ch);
                    }
                }
                best.and_then(|x| x.generator)
            }
            hk::Node::Selector {
                generators,
                selected,
            } => usize::try_from(selected)
                .ok()
                .and_then(|i| generators.get(i))
                .copied(),
            hk::Node::Reference { .. } => return ("a behaviour reference".into(), n),
            hk::Node::Swap => return ("a swap generator".into(), n),
            hk::Node::Clip(_) => return ("a clip generator".into(), n),
            hk::Node::Unread { class } => return (format!("an unread class {class}"), n),
        };
        match next {
            Some(x) => at = x,
            None => return ("a missing generator link".into(), n),
        }
    }
}

fn length_bucket(n: usize) -> String {
    match n {
        0..=4 => format!("{n}"),
        5..=9 => "5-9".into(),
        10..=19 => "10-19".into(),
        _ => "20 or more".into(),
    }
}

fn run() -> Result<(), E> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 4 {
        return Err("char_start_clip_census MANIFEST TIMELINE OUT.txt NAMES.txt".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let max = m.limits.max_memory;
    let timeline = Timeline::parse(&std::fs::read_to_string(&a[1])?)?;
    let mut c = Counts::new();
    let mut names = String::new();

    // 0: the actors and their races.
    let mut keys: Vec<FormKey> = Vec::new();
    for e in &timeline.events {
        if let What::Actor { reference, .. } = &e.what {
            let k = tl::norm(reference);
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
    }
    add(&mut c, "0 actors the timeline places", keys.len() as u64);
    let mut loader = Loader::new(
        &store,
        &vfs,
        &m.language,
        RuleLevel::Hypotheses,
        &Budget::new(max),
    );
    struct Resolved {
        key: FormKey,
        base: Option<FormKey>,
        race: FormKey,
        section: usize,
        skeleton: Vec<u8>,
    }
    let mut actors: Vec<Resolved> = Vec::new();
    let _ = writeln!(names, "# actors: key, base, race, section");
    for k in &keys {
        let b = Budget::new(max);
        let x = actor::resolve_reference(&mut loader, k, &SourceSpan::new("timeline", 0, 0), &b)?;
        match &x.plan {
            Some(p) => {
                bump(&mut c, "0 actors resolved to a race and a section");
                let _ = writeln!(
                    names,
                    "{}\t{:?}\t{}\t{}",
                    key_text(k),
                    x.base.as_ref().map(key_text),
                    key_text(&p.race),
                    p.section
                );
                actors.push(Resolved {
                    key: k.clone(),
                    base: x.base.clone(),
                    race: p.race.clone(),
                    section: p.section,
                    skeleton: p.skeleton.clone(),
                });
            }
            None => bump(
                &mut c,
                "0 actors not resolved (no plan: the cast step's reasons)",
            ),
        }
    }
    let races: BTreeSet<FormKey> = actors.iter().map(|x| x.race.clone()).collect();
    add(&mut c, "0 distinct races", races.len() as u64);
    let sections: BTreeSet<(FormKey, usize)> =
        actors.iter().map(|x| (x.race.clone(), x.section)).collect();
    add(&mut c, "0 distinct race sections", sections.len() as u64);

    // 1: the graph files.
    let mut files: BTreeMap<String, Via> = BTreeMap::new();
    let mut projects: BTreeMap<String, String> = BTreeMap::new();
    let mut entries_by_race: BTreeMap<FormKey, (FormKey, Vec<char_graph::Entry>)> = BTreeMap::new();
    let _ = writeln!(names, "# race sections: race, section, project, root graph");
    for (race, section) in &sections {
        let b = Budget::new(max);
        let Some(r) = store.raw(race, &b)? else {
            bump(&mut c, "1 race sections whose race has no record");
            continue;
        };
        let Some(p) = char_graph::race_project(&r, *section) else {
            bump(&mut c, "1 race sections without a project text after NAM3");
            continue;
        };
        match char_graph::project_chain(&vfs, &p, &b) {
            Ok((pk, bk)) => {
                bump(
                    &mut c,
                    "1 race sections whose project, character and graph resolve",
                );
                let _ = writeln!(names, "{}\t{section}\t{pk}\t{bk}", key_text(race));
                projects.insert(bk.clone(), pk);
                files.entry(bk).or_insert(Via::Root);
            }
            Err(e) => {
                bump(&mut c, "1 race sections whose chain does not resolve");
                let _ = writeln!(names, "{}\t{section}\t{p}\t{e}", key_text(race));
            }
        }
        if let Some(x) = char_graph::subgraphs(&store, race, &b)? {
            entries_by_race.insert(race.clone(), x);
        }
    }
    let distinct_projects: BTreeSet<&String> = projects.values().collect();
    add(
        &mut c,
        "1 distinct projects",
        distinct_projects.len() as u64,
    );
    let project_folder = distinct_projects
        .iter()
        .next()
        .map(|p| folder_of(p).to_string())
        .unwrap_or_default();
    // Subgraph entries' files.
    for (_, entries) in entries_by_race.values() {
        for e in entries {
            if let Some(k) = key_under("meshes", &e.behavior) {
                files.entry(k).or_insert(Via::Subgraph);
            }
        }
    }
    // Behaviour references, transitively (relative to the project folder).
    let mut queue: Vec<String> = files.keys().cloned().collect();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut missing_files = 0u64;
    while let Some(f) = queue.pop() {
        if !seen.insert(f.clone()) {
            continue;
        }
        let b = Budget::new(max);
        let Ok(bytes) = vfs.read(&f, &b) else {
            missing_files += 1;
            continue;
        };
        let Ok(hc) = hk::Container::read(&bytes, &f, 0, &b) else {
            continue;
        };
        for (i, o) in hc.objects.iter().enumerate() {
            if o.class != hk::BEHAVIOR_REFERENCE {
                continue;
            }
            if let Ok(hk::Node::Reference { behavior }) = hk::Node::read(&hc, i, &f, 0, &b) {
                if let Some(k) = key_under(&project_folder, &behavior) {
                    if !files.contains_key(&k) {
                        files.insert(k.clone(), Via::Reference);
                        queue.push(k);
                    }
                }
            }
        }
    }
    add(&mut c, "1 graph files named but not mounted", missing_files);
    for v in files.values() {
        bump(
            &mut c,
            format!("1 graph files by how they are reached\t{}", v.text()),
        );
    }
    // Which entries name each subgraph file (for clip resolution).
    let mut entries_of: BTreeMap<String, Vec<char_graph::Entry>> = BTreeMap::new();
    for (_, entries) in entries_by_race.values() {
        for e in entries {
            if let Some(k) = key_under("meshes", &e.behavior) {
                entries_of.entry(k).or_default().push(e.clone());
            }
        }
    }

    // 2-8 over the files.
    let tracked: BTreeSet<&str> = [
        hk::STATE_MACHINE,
        hk::STATE_INFO,
        hk::TRANSITIONS,
        hk::CLIP_GENERATOR,
        hk::BEHAVIOR_REFERENCE,
        hk::BLENDER,
        hk::POSE_MATCHING,
        hk::BLENDER_CHILD,
        hk::MANUAL_SELECTOR,
        hk::SWAP_GENERATOR,
        hk::MODIFIER_GENERATOR,
        hk::STATE_TAGGING,
        hk::DYNAMIC_TAGGING,
        hk::OFFSET_GENERATOR,
        hk::BINDING_SET,
        hk::VALUE_SET,
    ]
    .into_iter()
    .collect();
    let _ = writeln!(names, "# graph files: file, how reached, walk end, length");
    for (f, via) in &files {
        let b = Budget::new(max);
        let Ok(bytes) = vfs.read(f, &b) else { continue };
        let Ok(hc) = hk::Container::read(&bytes, f, 0, &b) else {
            bump(&mut c, "1 graph files the container reader refuses");
            continue;
        };
        bump(&mut c, "1 graph files read");
        let mut other_generators: BTreeSet<&str> = BTreeSet::new();
        for o in &hc.objects {
            if tracked.contains(o.class) {
                bump(&mut c, format!("1 objects by class\t{}", o.class));
            } else if o.class.ends_with("Generator") {
                other_generators.insert(o.class);
                bump(&mut c, "1 objects of other generator classes (not read)");
            }
        }
        for g in other_generators {
            bump(
                &mut c,
                format!("1 graph files holding another generator class (not read)\t{g}"),
            );
        }
        let vars = hk::graph_data(&hc, f, 0).and_then(|d| hk::Variables::read(&hc, d, f, 0, &b));
        let vars = match vars {
            Ok(v) => Some(v),
            Err(e) => {
                bump(
                    &mut c,
                    format!("7 graph files whose variables are refused\t{}", e.code),
                );
                None
            }
        };
        if let Some(v) = &vars {
            add(&mut c, "7 variables", v.types.len() as u64);
            for (t, w) in v.types.iter().zip(&v.words) {
                bump(&mut c, format!("7 variables by type byte\t{t}"));
                if *w != 0 {
                    bump(
                        &mut c,
                        format!(
                            "7 variables with a non-zero stored initial word, by type byte\t{t}"
                        ),
                    );
                }
            }
        }
        for (i, o) in hc.objects.iter().enumerate() {
            let binds = if tracked.contains(o.class) {
                hk::bindings(&hc, i, f, 0, &b).unwrap_or_default()
            } else {
                Vec::new()
            };
            let Ok(node) = hk::Node::read(&hc, i, f, 0, &b) else {
                if o.class.ends_with("Generator") || o.class == hk::STATE_MACHINE {
                    bump(
                        &mut c,
                        format!("1 generators the reader refuses\t{}", o.class),
                    );
                }
                continue;
            };
            match node {
                hk::Node::Machine(sm) => {
                    bump(&mut c, "2 state machines");
                    add(&mut c, "2 states", sm.states.len() as u64);
                    let mut ids = Vec::new();
                    let mut local = 0u64;
                    let mut eventless = 0u64;
                    let mut conditions = 0u64;
                    for &s in &sm.states {
                        if let Ok(si) = hk::StateInfo::read(&hc, s, f, 0) {
                            ids.push(si.id);
                            if let Some(t) = si.transitions {
                                if let Ok(ts) = hk::transitions(&hc, t, f, 0, &b) {
                                    local += ts.len() as u64;
                                    eventless += ts.iter().filter(|x| x.event < 0).count() as u64;
                                    conditions +=
                                        ts.iter().filter(|x| x.condition.is_some()).count() as u64;
                                }
                            }
                        }
                    }
                    let mut wild = 0u64;
                    if let Some(t) = sm.wildcards {
                        if let Ok(ts) = hk::transitions(&hc, t, f, 0, &b) {
                            wild += ts.len() as u64;
                            eventless += ts.iter().filter(|x| x.event < 0).count() as u64;
                            conditions +=
                                ts.iter().filter(|x| x.condition.is_some()).count() as u64;
                        }
                    }
                    add(&mut c, "2 transitions out of a state", local);
                    add(&mut c, "2 wildcard transitions", wild);
                    add(
                        &mut c,
                        "2 transitions with no event (index below 0)",
                        eventless,
                    );
                    add(&mut c, "2 transitions with a condition link", conditions);
                    bump(
                        &mut c,
                        format!(
                            "2 state machines whose stored start state is one of their states\t{}",
                            ids.contains(&sm.start)
                        ),
                    );
                    match binds.iter().find(|x| x.path == "startStateId") {
                        None => bump(&mut c, "2 state machines by start state\tstored"),
                        Some(x) => {
                            let t = vars
                                .as_ref()
                                .and_then(|v| v.types.get(x.variable.max(0) as usize).copied());
                            bump(
                                &mut c,
                                format!(
                                    "2 state machines by start state\tbound to a {} of type byte {t:?}",
                                    if x.kind == 0 { "variable" } else { "character property" }
                                ),
                            );
                            if let (Some(v), 0) = (&vars, x.kind) {
                                let n = v.number(x.variable).map(|n| n as i32);
                                bump(
                                    &mut c,
                                    format!(
                                        "2 bound start states whose variable's stored value is one of the machine's states\t{}",
                                        n.is_some_and(|n| ids.contains(&n))
                                    ),
                                );
                            }
                        }
                    }
                }
                hk::Node::Clip(g) => {
                    bump(&mut c, "3 clip generators");
                    bump(
                        &mut c,
                        format!("3 clip generators by mode byte\t{}", g.mode),
                    );
                    bump(
                        &mut c,
                        format!(
                            "3 clip generators by start time\t{}",
                            if g.start_time == 0.0 { "0" } else { "not 0" }
                        ),
                    );
                    bump(
                        &mut c,
                        format!(
                            "3 clip generators by playback speed\t{}",
                            if g.playback_speed == 1.0 {
                                "1"
                            } else {
                                "not 1"
                            }
                        ),
                    );
                    bump(
                        &mut c,
                        format!(
                            "3 clip generators by crop\t{}",
                            if g.crop_start == 0.0 && g.crop_end == 0.0 {
                                "none"
                            } else {
                                "some"
                            }
                        ),
                    );
                    let parts: Vec<&str> = g.clip.split('\\').collect();
                    bump(
                        &mut c,
                        format!(
                            "3 clip texts by first part and number of parts\t{} {}",
                            if parts
                                .first()
                                .is_some_and(|p| p.eq_ignore_ascii_case("animations"))
                            {
                                "Animations"
                            } else {
                                "other"
                            },
                            parts.len()
                        ),
                    );
                    bump(
                        &mut c,
                        format!(
                            "3 clip texts by extension\t{}",
                            g.clip
                                .rsplit_once('.')
                                .map_or("none".into(), |(_, e)| e.to_ascii_lowercase())
                        ),
                    );
                    let stem = g.clip.rsplit_once('.').map_or(g.clip.as_str(), |(s, _)| s);
                    let name = stem.rsplit('\\').next().unwrap_or(stem);
                    let below = stem.split_once('\\').map_or(stem, |(_, r)| r);
                    match entries_of.get(f) {
                        Some(es) if *via == Via::Subgraph || !es.is_empty() => {
                            for e in es {
                                let found = |rel: &str| {
                                    e.folders.iter().any(|fo| {
                                        key_under("meshes", &format!("{fo}\\{rel}.hkx"))
                                            .is_some_and(|k| exists(&vfs, &k))
                                    })
                                };
                                bump(&mut c, format!("3 clip generator and subgraph entry pairs: the file name in one of the entry's folders\t{}", found(name)));
                                bump(&mut c, format!("3 clip generator and subgraph entry pairs: the text below its first part in one of the entry's folders\t{}", found(below)));
                            }
                        }
                        _ => {
                            let k = key_under(&project_folder, &format!("{stem}.hkx"));
                            bump(
                                &mut c,
                                format!(
                                    "3 clip generators outside subgraph files: the text with .hkx under the project folder mounted\t{}",
                                    k.is_some_and(|k| exists(&vfs, &k))
                                ),
                            );
                        }
                    }
                }
                hk::Node::Reference { behavior } => {
                    bump(&mut c, "4 behaviour references");
                    let k = key_under(&project_folder, &behavior);
                    bump(
                        &mut c,
                        format!(
                            "4 behaviour references whose text under the project folder is mounted\t{}",
                            k.is_some_and(|k| exists(&vfs, &k))
                        ),
                    );
                }
                hk::Node::Blender {
                    flags, children, ..
                } => {
                    bump(&mut c, "5 blenders");
                    add(&mut c, "5 blender children", children.len() as u64);
                    bump(&mut c, format!("5 blenders by flag word\t{flags:#06x}"));
                    let outside = children.iter().any(|x| !(0.0..=1.0).contains(&x.weight));
                    bump(
                        &mut c,
                        format!(
                            "5 blenders by flag bit 0x10 and a child weight outside 0 to 1\t{} {}",
                            flags & char_graph::BLEND_PARAMETRIC_FLAG != 0,
                            outside
                        ),
                    );
                    let most = children
                        .iter()
                        .map(|x| x.weight)
                        .fold(f32::NEG_INFINITY, f32::max);
                    let at_most = children.iter().filter(|x| x.weight == most).count();
                    bump(
                        &mut c,
                        format!(
                            "5 blenders by children sharing the highest stored weight\t{}",
                            at_most.min(3)
                        ),
                    );
                    for ch in &children {
                        if let Ok(bs) = hk::bindings(&hc, ch.object, f, 0, &b) {
                            if bs.iter().any(|x| x.path == "weight") {
                                bump(&mut c, "5 blender children whose weight is bound");
                            }
                        }
                    }
                }
                hk::Node::Selector {
                    generators,
                    selected,
                } => {
                    bump(&mut c, "6 manual selectors");
                    bump(
                        &mut c,
                        format!(
                            "6 manual selectors by index\t{}",
                            if binds.iter().any(|x| x.path == "selectedGeneratorIndex") {
                                "bound"
                            } else {
                                "stored"
                            }
                        ),
                    );
                    bump(
                        &mut c,
                        format!(
                            "6 manual selectors whose stored index is one of their generators\t{}",
                            usize::try_from(selected).is_ok_and(|i| i < generators.len())
                        ),
                    );
                }
                hk::Node::Swap => bump(&mut c, "1 swap generators"),
                _ => {}
            }
        }
        let (end, n) = file_walk(&hc, f, &b);
        bump(
            &mut c,
            format!("8 graph files by where the stored path from the root ends\t{end}"),
        );
        bump(
            &mut c,
            format!(
                "8 graph files by generators on the stored path from the root to a clip\t{}",
                if end == "a clip generator" {
                    length_bucket(n)
                } else {
                    "no clip".into()
                }
            ),
        );
        let _ = writeln!(names, "{f}\t{}\t{end}\t{n}", via.text());
    }

    // 9: subgraph entries.
    let _ = writeln!(names, "# subgraph races: race, subgraph race, entries");
    for (race, (sub, entries)) in &entries_by_race {
        let _ = writeln!(
            names,
            "{}\t{}\t{}",
            key_text(race),
            key_text(sub),
            entries.len()
        );
        bump(
            &mut c,
            format!(
                "9 races of the actors by where their subgraph entries are\t{}",
                if race == sub {
                    "the race itself"
                } else {
                    "the race SRAC names"
                }
            ),
        );
        add(&mut c, "9 subgraph entries", entries.len() as u64);
        for e in entries {
            bump(
                &mut c,
                format!("9 subgraph entries by flag word\t{:#010x}", e.flags),
            );
            if e.actor_keywords.is_empty() && e.target_keywords.is_empty() {
                bump(
                    &mut c,
                    format!(
                        "9 subgraph entries without keywords, by flag word\t{:#010x}",
                        e.flags
                    ),
                );
            }
            if e.flags == 0 {
                bump(
                    &mut c,
                    format!(
                        "9 entries of flag word 0 by actor and target keyword counts\t{} {}",
                        e.actor_keywords.len().min(4),
                        e.target_keywords.len().min(4)
                    ),
                );
            }
            bump(
                &mut c,
                format!(
                    "9 subgraph entries whose behaviour file is mounted\t{}",
                    key_under("meshes", &e.behavior).is_some_and(|k| exists(&vfs, &k))
                ),
            );
        }
    }
    let _ = writeln!(
        names,
        "# actors' keywords: actor, keywords or why not, entry picked"
    );
    let mut keywords_of: BTreeMap<FormKey, Result<BTreeSet<FormKey>, String>> = BTreeMap::new();
    for x in &actors {
        let b = Budget::new(max);
        let kw = match &x.base {
            Some(base) => char_graph::actor_keywords(&store, base, &x.race, &b)?,
            None => Err("no base".into()),
        };
        match &kw {
            Ok(k) => {
                bump(&mut c, "9 actors whose keywords are read");
                bump(
                    &mut c,
                    format!("9 actors by own keyword count\t{}", k.len().min(9)),
                );
                if let Some((_, entries)) = entries_by_race.get(&x.race) {
                    let fit = entries
                        .iter()
                        .filter(|e| e.flags == 0 && e.target_keywords.is_empty())
                        .filter(|e| e.actor_keywords.iter().all(|y| k.contains(y)))
                        .collect::<Vec<_>>();
                    let with = fit.iter().filter(|e| !e.actor_keywords.is_empty()).count();
                    bump(
                        &mut c,
                        format!("9 actors by entries of flag word 0 their keywords fit\t{} ({} with keywords)", fit.len(), with),
                    );
                    let pick = char_graph::pick_entry(entries, k);
                    bump(
                        &mut c,
                        format!(
                            "9 actors by the entry picked\t{}",
                            match &pick {
                                Ok(e) if e.actor_keywords.is_empty() =>
                                    "the entry without keywords".to_string(),
                                Ok(_) => "an entry with keywords".into(),
                                Err(_) => "none (refused)".into(),
                            }
                        ),
                    );
                    let _ = writeln!(
                        names,
                        "{}\t{} keywords\t{:?}",
                        key_text(&x.key),
                        k.len(),
                        pick.map(|e| (e.index, e.behavior.clone()))
                    );
                }
            }
            Err(why) => {
                bump(
                    &mut c,
                    format!("9 actors whose keywords are not read\t{why}"),
                );
                let _ = writeln!(names, "{}\t{why}", key_text(&x.key));
            }
        }
        keywords_of.insert(x.key.clone(), kw);
    }

    // 10: the walk with every switch on.
    let on = Switches {
        start: true,
        subgraph: true,
        folder: true,
        cycle: true,
    };
    let _ = writeln!(names, "# walks: actor, outcome");
    let mut clips: BTreeMap<String, (f64, u8, bool)> = BTreeMap::new();
    for x in &actors {
        let b = Budget::new(max);
        let kw = keywords_of.get(&x.key).expect("read above");
        let input = char_graph::Actor {
            race: &x.race,
            section: x.section,
            keywords: kw.as_ref().map_err(|s| s.as_str()),
        };
        match char_graph::start_clip(&vfs, &store, &input, &on, &b)? {
            Ok(s) => {
                bump(&mut c, "10 actors whose walk reaches a clip");
                bump(
                    &mut c,
                    format!(
                        "10 walks reaching a clip by generators on the path\t{}",
                        length_bucket(s.steps.len())
                    ),
                );
                bump(
                    &mut c,
                    format!(
                        "10 walks reaching a clip by graph files opened\t{}",
                        s.steps
                            .iter()
                            .map(|x| &x.file)
                            .collect::<BTreeSet<_>>()
                            .len()
                    ),
                );
                bump(
                    &mut c,
                    format!(
                        "10 walks reaching a clip by blenders taken at a tie\t{}",
                        s.ties
                    ),
                );
                bump(
                    &mut c,
                    format!(
                        "10 walks reaching a clip by the clip generator's mode byte\t{}",
                        s.generator.mode
                    ),
                );
                bump(
                    &mut c,
                    format!(
                        "10 walks reaching a clip by start time\t{}",
                        s.generator.start_time
                    ),
                );
                for st in &s.steps {
                    bump(
                        &mut c,
                        format!("10 generators on the reaching paths by class\t{}", st.class),
                    );
                }
                let entry = clips.entry(s.clip.clone()).or_insert_with(|| {
                    let read = vfs
                        .read(&s.clip, &b)
                        .and_then(|bytes| actor::clip::Clip::read(&bytes, &s.clip, &b));
                    let skel = actor::clip::skeleton_hkx_path(&x.skeleton)
                        .map(|p| String::from_utf8_lossy(&p).into_owned())
                        .and_then(|p| {
                            vfs.read(&p, &b).ok().and_then(|bytes| {
                                actor::clip::HkSkeleton::read(&bytes, &p, &b).ok()
                            })
                        });
                    match read {
                        Ok(cl) => (
                            cl.duration(),
                            s.generator.mode,
                            skel.is_some_and(|h| cl.fits(&h)),
                        ),
                        Err(_) => (-1.0, s.generator.mode, false),
                    }
                });
                let _ = writeln!(
                    names,
                    "{}\tclip {}\t{:?}\tlength {:.3}\tfits {}",
                    key_text(&x.key),
                    s.clip,
                    s.subgraph.as_ref().map(|e| e.index),
                    entry.0,
                    entry.2
                );
            }
            Err(r) => {
                bump(
                    &mut c,
                    format!("10 actors whose walk is refused, by code\t{}", r.code),
                );
                let _ = writeln!(
                    names,
                    "{}\trefused {} {}",
                    key_text(&x.key),
                    r.code,
                    r.detail
                );
            }
        }
    }
    add(&mut c, "10 distinct clips reached", clips.len() as u64);
    for (len, mode, fits) in clips.values() {
        bump(
            &mut c,
            format!(
                "10 distinct clips reached by mode byte, length and fit to the actor's skeleton\t{mode} {} {fits}",
                if *len < 0.0 { "unreadable".into() } else { format!("{len:.2} s") }
            ),
        );
    }

    // 12: sitting: what the timeline says and what furniture data gives.
    for e in &timeline.events {
        let kind = match &e.what {
            What::Player { .. } => "player",
            What::Actor { .. } => "actor (a place)",
            What::Visibility { .. } => "visibility",
            What::Move { .. } => "move",
            What::Stage { .. } => "stage",
            What::Objective { .. } => "objective",
            What::Quest { .. } => "quest",
            What::Scene { .. } => "scene",
            What::Line(_) => "line",
            What::Note(_) => "note",
        };
        bump(
            &mut c,
            format!("12 timeline events by kind (none says who sits on what)\t{kind}"),
        );
    }
    let first_race = actors
        .iter()
        .map(|x| x.race.clone())
        .find(|r| entries_by_race.contains_key(r));
    if let Some((_, entries)) = first_race.as_ref().and_then(|r| entries_by_race.get(r)) {
        let furniture: Vec<FormKey> = store
            .winners()
            .filter(|w| !w.deleted && w.signature == *b"FURN")
            .map(|w| store.overrides().form_key(w.key, store.order()))
            .collect();
        let target = |e: &&char_graph::Entry| (e.flags & 0xffff) == 2 && e.flags >> 16 == 0;
        add(
            &mut c,
            "12 subgraph entries of flag word 2 (their behaviour files' names say furniture in most) of the first actor race's subgraph race",
            entries.iter().filter(target).count() as u64,
        );
        for f in &furniture {
            let b = Budget::new(max);
            let Some((r, p)) = store.raw_with_plugin(f, &b)? else {
                continue;
            };
            bump(&mut c, "12 winning furniture records (FURN)");
            let kw: BTreeSet<FormKey> = char_graph::record_keywords(&store, &r, p)?
                .into_iter()
                .collect();
            let fits: Vec<&char_graph::Entry> = entries
                .iter()
                .filter(target)
                .filter(|e| {
                    !e.target_keywords.is_empty()
                        && e.target_keywords.iter().all(|k| kw.contains(k))
                })
                .collect();
            let no_actor = fits.iter().filter(|e| e.actor_keywords.is_empty()).count();
            let most = fits
                .iter()
                .filter(|e| e.actor_keywords.is_empty())
                .map(|e| e.target_keywords.len())
                .max();
            let best = most.map_or(0, |m| {
                fits.iter()
                    .filter(|e| e.actor_keywords.is_empty() && e.target_keywords.len() == m)
                    .count()
            });
            bump(
                &mut c,
                format!(
                    "12 furniture records by flag-word-2 entries whose target keywords all are the furniture's	{}",
                    match (fits.len(), no_actor, best) {
                        (0, _, _) => "none".to_string(),
                        (_, 0, _) => "only entries that also need actor keywords".into(),
                        (_, _, 1) => "one entry without actor keywords has the most".into(),
                        (_, _, n) => format!("{n} entries without actor keywords tie"),
                    }
                ),
            );
        }
    }

    // 11: layout checks over every behaviour graph file.
    let all: BTreeSet<String> = vfs
        .candidates
        .iter()
        .filter_map(|x| x.key.as_ref())
        .filter(|k| k.ends_with(b".hkx"))
        .map(|k| String::from_utf8_lossy(k).into_owned())
        .collect();
    for f in &all {
        let b = Budget::new(max);
        let Ok(bytes) = vfs.read(f, &b) else { continue };
        let Ok(hc) = hk::Container::read(&bytes, f, 0, &b) else {
            continue;
        };
        if hk::only_object(&hc, hk::BEHAVIOR_GRAPH).is_none() {
            continue;
        }
        bump(&mut c, "11 behaviour graph files of the installation");
        match hk::graph_data(&hc, f, 0).and_then(|d| hk::Variables::read(&hc, d, f, 0, &b)) {
            Ok(v) => {
                bump(
                    &mut c,
                    "11 graph files whose variable types, initial words and names agree in count",
                );
                let wide = v.types.iter().filter(|t| (6..=8).contains(*t)).count() as u32;
                let ptrs = v.types.iter().filter(|t| **t == 5).count() as u32;
                bump(&mut c, format!("11 graph files whose 16-byte value count equals their variables of type byte 6 to 8\t{}", v.quads == wide));
                bump(&mut c, format!("11 graph files whose link value count equals their variables of type byte 5\t{}", v.variants == ptrs));
                for (i, o) in hc.objects.iter().enumerate() {
                    if o.class == hk::BINDING_SET {
                        continue;
                    }
                    let Ok(bs) = hk::bindings(&hc, i, f, 0, &b) else {
                        if o.class.starts_with("hkb") || o.class.starts_with("BS") {
                            bump(&mut c, "11 nodes whose binding set the reader refuses");
                        }
                        continue;
                    };
                    for x in bs {
                        bump(&mut c, format!("11 bindings by kind byte\t{}", x.kind));
                        bump(
                            &mut c,
                            format!(
                                "11 bindings by bit byte\t{}",
                                if x.bit == -1 { "-1" } else { "other" }
                            ),
                        );
                        if x.kind == 0 {
                            bump(
                                &mut c,
                                format!("11 kind-0 bindings whose index is one of the graph's variables\t{}", usize::try_from(x.variable).is_ok_and(|k| k < v.types.len())),
                            );
                        }
                        if x.path == "startStateId" {
                            bump(
                                &mut c,
                                format!(
                                    "11 bound start states by the variable's type byte\t{:?}",
                                    v.types.get(x.variable.max(0) as usize)
                                ),
                            );
                        }
                    }
                }
            }
            Err(e) => bump(
                &mut c,
                format!("11 graph files whose variables are refused\t{}", e.code),
            ),
        }
        for (i, o) in hc.objects.iter().enumerate() {
            let generator = o.class.ends_with("Generator") || o.class == hk::STATE_MACHINE;
            if !generator {
                continue;
            }
            match hk::Node::read(&hc, i, f, 0, &b) {
                Ok(hk::Node::Unread { .. }) => bump(&mut c, "11 generators of classes not read"),
                Ok(hk::Node::Clip(g)) => {
                    bump(
                        &mut c,
                        format!("11 clip generators by mode byte\t{}", g.mode),
                    );
                    bump(
                        &mut c,
                        format!("11 clip generators by binding index\t{}", g.binding_index),
                    );
                }
                Ok(hk::Node::Selector {
                    generators,
                    selected,
                }) => bump(
                    &mut c,
                    format!(
                        "11 manual selectors whose stored index is one of their generators\t{}",
                        usize::try_from(selected).is_ok_and(|k| k < generators.len())
                    ),
                ),
                Ok(hk::Node::Blender {
                    flags, children, ..
                }) => {
                    let outside = children.iter().any(|x| !(0.0..=1.0).contains(&x.weight));
                    bump(
                        &mut c,
                        format!("11 blenders by flag bit 0x10 and a child weight outside 0 to 1\t{} {outside}", flags & char_graph::BLEND_PARAMETRIC_FLAG != 0),
                    );
                }
                Ok(hk::Node::Swap) => bump(&mut c, "11 swap generators"),
                Ok(_) => bump(&mut c, "11 other generators read"),
                Err(e) => bump(
                    &mut c,
                    format!("11 generators the reader refuses\t{} {}", o.class, e.code),
                ),
            }
        }
    }

    let mut out = String::from(
        "# char_start_clip_census: kinds and counts only (spec/character-graph.md). Sections: 0 the run's actors; 1 their races' graph files and objects by class; 2 state machines; 3 clip generators; 4 behaviour references; 5 blenders; 6 manual selectors; 7 variables;\n# 8 the stored path from each file's root; 9 subgraph entries and the actors' keywords; 10 the walk with every switch on; 11 layout checks over every behaviour graph file of the installation; 12 sitting: the timeline's event kinds and the furniture records' subgraph entries.\n",
    );
    for (k, v) in &c {
        let _ = writeln!(out, "{v}\t{k}");
    }
    std::fs::write(&a[2], out)?;
    std::fs::write(&a[3], names)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
