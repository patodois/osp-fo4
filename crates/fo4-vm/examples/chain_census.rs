#![forbid(unsafe_code)]
//! What the data holds behind the stops of the chain step (`spec/vm.md`,
//! The town chain): `chain_census MANIFEST OUT.txt NAMES.txt
//! [--quest PLUGIN:OBJECTID]... [--chain PLUGIN:OBJECTID:STAGE]...
//! [--stage PLUGIN:OBJECTID:STAGE]... [--line PLUGIN:OBJECTID]...`.
//!
//! OUT.txt holds counts and kinds only (`spec/quest-run-chain-census.txt`);
//! NAMES.txt names records and scripts and is never versioned. Nothing
//! runs. Sections:
//!
//! 1. winning records whose record flag word has bit 0x4000: by record
//!    type, whether an earlier occurrence of the record exists, the
//!    subrecord tags they carry and the tags the occurrence before them
//!    carries that they do not; for quests, whether they hold stages and
//!    whether the occurrence before holds some;
//! 2. constant stage setters (`fo4_vm::setters`, outer calls followed)
//!    naming a stage the winning quest record does not hold: by kind,
//!    fragments apart, and whether the occurrence before a flag-0x4000
//!    winner holds that stage;
//! 3. each `--quest` (numbered in the order given): its flag word and
//!    story event, the story manager quest nodes listing it, the ways the
//!    setter table starts it, and the setters of its stages by kind and
//!    need (the stages as the occurrence before a flag-0x4000 winner
//!    holds them); each `--chain` (numbered): the setters of that stage,
//!    and of the stages they need, up to four steps back, by how each
//!    path ends;
//! 4. the container change event: the scripts defining its handler with
//!    code, the attachments whose chain handles it by kind, by whether the
//!    handler's script serves ten attachments or more and by whether they
//!    store an integer property, and whether their chain adds an inventory
//!    filter;
//! 5. each `--stage` (numbered): the setters naming its quest whose stage
//!    the scan does not resolve, the attachments storing a property that
//!    names the quest (by kind; whether their script chain calls a set
//!    stage function at all; whether they store an integer equal to the
//!    stage), and the setters with an unresolved quest, by reason;
//! 6. each `--line` (numbered): its topic, the topic's other lines and
//!    their conditions by function, the winning records holding a word
//!    that names the topic or the line, by record type and tag, and for
//!    each scene naming the topic its conditions and the words naming it;
//! 7. locations: the winning location records with a parent (`PNAM`), the
//!    links up to one without, the `IsChild` call sites, and the scripts
//!    and attachments handling the location change event.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{
    actor::Context, condition::lists, quest::Quest, story::Tree, typed::Store, FormKey, Record,
};
use fo4_vfs::Vfs;
use fo4_vm::{
    bind::record_key,
    setters::{self, Need, Resolved, Setter},
    Lit, MapLibrary,
};
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

/// The record flag bit this census follows.
const PARTIAL: u32 = 0x4000;

fn sig(s: [u8; 4]) -> String {
    String::from_utf8_lossy(&s).to_string()
}

fn tags(r: &Record) -> BTreeSet<String> {
    r.subrecords.iter().map(|s| sig(s.signature)).collect()
}

/// The occurrence before the winning one, read again, with its plugin.
fn previous(store: &Store, k: &FormKey, b: &Budget) -> Option<(Record, usize)> {
    let w = store.overrides().winner(k)?;
    let p = w.previous? as usize;
    store.raw_occurrence(p, b).ok()
}

/// The stages a quest record holds.
fn stages_of(store: &Store, r: &Record, plugin: usize, b: &Budget) -> Option<BTreeSet<i32>> {
    let ctx = Context {
        order: store.order(),
        plugin,
    };
    Quest::read(r, ctx, b)
        .ok()
        .map(|q| q.stages.iter().map(|s| i32::from(s.stage)).collect())
}

/// Whether a script of the chain calls a function of one of these names
/// (opcode 23, the function name its first argument).
fn chain_calls(lib: &MapLibrary, script: &str, names: &[&str]) -> bool {
    let mut t = script.to_string();
    for _ in 0..fo4_vm::MAX_PARENTS {
        let Some(s) = lib.scripts.get(&t) else {
            return false;
        };
        if s.functions.values().any(|f| {
            f.code.iter().any(|i| {
                i.op == 23
                    && matches!(i.args.first(), Some(Lit::Ident(n)) if names.contains(&n.as_str()))
            })
        }) {
            return true;
        }
        if s.parent.is_empty() {
            return false;
        }
        t = s.parent.clone();
    }
    false
}

/// The script of the chain that defines a handler with code, if any.
fn handler_in_chain(lib: &MapLibrary, script: &str, event: &str) -> Option<String> {
    let mut t = script.to_string();
    for _ in 0..fo4_vm::MAX_PARENTS {
        let s = lib.scripts.get(&t)?;
        if s.functions
            .iter()
            .any(|((_, n), f)| n == event && !f.code.is_empty())
        {
            return Some(t);
        }
        if s.parent.is_empty() {
            return None;
        }
        t = s.parent.clone();
    }
    None
}

fn parse_stage(v: &str) -> Option<(FormKey, i32)> {
    let (k, n) = v.rsplit_once(':')?;
    Some((fo4_vm::player::parse_key(k)?, n.parse().ok()?))
}

fn run() -> Result<(), E> {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let usage = "chain_census MANIFEST OUT.txt NAMES.txt [--quest PLUGIN:OBJECTID]... [--chain PLUGIN:OBJECTID:STAGE]... [--stage PLUGIN:OBJECTID:STAGE]... [--line PLUGIN:OBJECTID]...";
    let mut quests: Vec<FormKey> = Vec::new();
    let mut chains: Vec<(FormKey, i32)> = Vec::new();
    let mut stages: Vec<(FormKey, i32)> = Vec::new();
    let mut lines: Vec<FormKey> = Vec::new();
    while let Some(i) = a
        .iter()
        .position(|x| matches!(x.as_str(), "--quest" | "--chain" | "--stage" | "--line"))
    {
        let v = a.get(i + 1).cloned().ok_or(usage)?;
        match a[i].as_str() {
            "--quest" => quests.push(fo4_vm::player::parse_key(&v).ok_or(usage)?),
            "--chain" => chains.push(parse_stage(&v).ok_or(usage)?),
            "--stage" => stages.push(parse_stage(&v).ok_or(usage)?),
            _ => lines.push(fo4_vm::player::parse_key(&v).ok_or(usage)?),
        }
        a.drain(i..i + 2);
    }
    if a.len() != 3 {
        return Err(usage.into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let max = m.limits.max_memory;
    let b = Budget::new(max);
    let lib = setters::read_all_scripts(&vfs, max)?;
    let (atts, _) = setters::attachments(&store, &setters::owners(&store), &b)?;
    let tree = Tree::read(&store, &b)?;
    let t = setters::table_with(&lib, &atts, &store, Some(&tree), &b, true)?;
    let label = |k: &FormKey| -> String {
        fo4_vm::bind::label(&store, k, &b.child()).unwrap_or_else(|_| record_key(k))
    };
    let mut c = Counts::new();
    let mut names = String::new();

    // 1 winning records with flag bit 0x4000.
    let p1 = "1 winning records with record flag bit 0x4000";
    // Quests whose winner carries the bit: the stages the occurrence before holds.
    let mut before_stages: BTreeMap<String, BTreeSet<i32>> = BTreeMap::new();
    let mut scanned = 0u64;
    for o in store.winners() {
        if o.deleted {
            continue;
        }
        let k = store.overrides().form_key(o.key, store.order());
        let bb = Budget::new(max);
        let Ok(Some((r, plugin))) = store.raw_with_plugin(&k, &bb) else {
            continue;
        };
        scanned += 1;
        if r.flags & PARTIAL == 0 {
            continue;
        }
        let s = sig(r.signature);
        bump(&mut c, format!("{p1}\tby record type\t{s}"));
        let own = tags(&r);
        bump(
            &mut c,
            format!(
                "{p1}\tby record type and the tags they carry\t{s}\t{}",
                own.iter().cloned().collect::<Vec<_>>().join(" ")
            ),
        );
        let prev = previous(&store, &k, &bb);
        match &prev {
            None => bump(&mut c, format!("{p1}\tno earlier occurrence\t{s}")),
            Some((pr, _)) => {
                bump(&mut c, format!("{p1}\twith an earlier occurrence\t{s}"));
                if pr.flags & PARTIAL != 0 {
                    bump(
                        &mut c,
                        format!("{p1}\tthe occurrence before carries the bit too\t{s}"),
                    );
                }
                let missing: Vec<String> = tags(pr).difference(&own).cloned().collect();
                bump(
                    &mut c,
                    format!(
                        "{p1}\tthe occurrence before carries tags they do not\t{s}\t{}",
                        if missing.is_empty() { "none" } else { "some" }
                    ),
                );
            }
        }
        if r.signature == *b"QUST" {
            let held = stages_of(&store, &r, plugin, &bb).unwrap_or_default();
            let before = prev
                .as_ref()
                .and_then(|(pr, pp)| stages_of(&store, pr, *pp, &bb))
                .unwrap_or_default();
            bump(
                &mut c,
                format!(
                    "{p1}\tquests: stages held, by them and by the occurrence before\t{}, {}",
                    if held.is_empty() { "none" } else { "some" },
                    if before.is_empty() { "none" } else { "some" }
                ),
            );
            let _ = writeln!(
                names,
                "## 1 quest {} carries bit 0x4000 ({} in {}): tags {:?}; stages {} held, {} before",
                label(&k),
                record_key(&k),
                r.source.file.rsplit(['/', '\\']).next().unwrap_or(""),
                own,
                held.len(),
                before.len()
            );
            before_stages.insert(record_key(&k), before);
        }
    }
    let _ = writeln!(names, "## 1 {scanned} winning records scanned");

    // 2 constant setters naming a stage the winning quest does not hold.
    let p2 = "2 constant setters naming a stage the winning quest record does not hold";
    for s in &t.setters {
        let (Some(q), Some(n)) = (s.quest.get(), s.stage.get()) else {
            continue;
        };
        let qk = record_key(q);
        let Some((_, held)) = t.stages.get(&qk) else {
            continue;
        };
        if held.iter().any(|x| i32::from(x.0) == *n) {
            continue;
        }
        let fragment = s.kind.contains("fragment") && s.depth == 0;
        bump(&mut c, format!("{p2}\tall"));
        bump(&mut c, format!("{p2}\tby kind\t{}", s.kind));
        if fragment {
            bump(&mut c, format!("{p2}\tfragments (a fragment's own call)"));
        }
        let before = before_stages.get(&qk);
        let what = match before {
            None => "the winner does not carry bit 0x4000",
            Some(v) if v.contains(n) => {
                "the winner carries bit 0x4000; the occurrence before holds the stage"
            }
            Some(_) => {
                "the winner carries bit 0x4000; the occurrence before does not hold the stage"
            }
        };
        bump(&mut c, format!("{p2}\tby the record\t{what}"));
        if fragment {
            bump(&mut c, format!("{p2}\tfragments by the record\t{what}"));
        }
        let _ = writeln!(
            names,
            "## 2 {} stage {n} | {} | {} | {what} | {}",
            label(q),
            s.kind,
            s.need.text(&store),
            s.site
        );
    }

    // 3 the quests named and the chains back from a stage.
    let mut quest_sets: BTreeMap<String, BTreeSet<i32>> = BTreeMap::new();
    for (qk, (_, v)) in &t.stages {
        quest_sets.insert(qk.clone(), v.iter().map(|x| i32::from(x.0)).collect());
    }
    for (n, q) in quests.iter().enumerate() {
        let p = format!("3 quest {}", n + 1);
        let qk = record_key(q);
        let _ = writeln!(names, "## 3 quest {} is {}", n + 1, label(q));
        if let Some((r, plugin)) = store.raw_with_plugin(q, &b)? {
            let partial = r.flags & PARTIAL != 0;
            let (qr, qp) = if partial {
                previous(&store, q, &b).unwrap_or((r, plugin))
            } else {
                (r, plugin)
            };
            let ctx = Context {
                order: store.order(),
                plugin: qp,
            };
            if let Ok(qq) = Quest::read(&qr, ctx, &b) {
                let from = if partial {
                    "the occurrence before the winner (bit 0x4000)"
                } else {
                    "the winner"
                };
                bump(
                    &mut c,
                    format!(
                        "{p}\tflag word (DNAM), read from {from}\t{}",
                        qq.general
                            .map(|g| format!("0x{:04x}", g.flags))
                            .unwrap_or("none".into())
                    ),
                );
                bump(
                    &mut c,
                    format!(
                        "{p}\tstarts with the game (flag bit 0x1)\t{}",
                        qq.general.is_some_and(|g| g.flags & 1 != 0)
                    ),
                );
                bump(
                    &mut c,
                    format!(
                        "{p}\tstory event (ENAM)\t{}",
                        qq.event.map(sig).unwrap_or("none".into())
                    ),
                );
                bump(&mut c, format!("{p}\tstages\t{}", qq.stages.len()));
                let startup = qq.stages.iter().filter(|s| s.flags & 0x02 != 0).count();
                bump(
                    &mut c,
                    format!("{p}\tstartup stages (INDX flag 0x2)\t{startup}"),
                );
                quest_sets.insert(
                    qk.clone(),
                    qq.stages.iter().map(|s| i32::from(s.stage)).collect(),
                );
            }
        }
        let mut listed = 0;
        for (nk, node) in &tree.nodes {
            if node.quest_keys().any(|x| x == q) {
                listed += 1;
                let ev = tree
                    .path(nk)
                    .and_then(|path| path.iter().find_map(|x| tree.event_of(x)));
                bump(
                    &mut c,
                    format!(
                        "{p}\tstory manager quest nodes listing it, by the event above them\t{}",
                        ev.map(sig).unwrap_or("none".into())
                    ),
                );
                let _ = writeln!(names, "    story node {} lists it", label(nk));
            }
        }
        if listed == 0 {
            bump(
                &mut c,
                format!("{p}\tstory manager quest nodes listing it\t0"),
            );
        }
        let starters = t.of_quest(q);
        bump(
            &mut c,
            format!("{p}\tstarts in the setter table\t{}", starters.len()),
        );
        for st in starters {
            bump(&mut c, format!("{p}\tstarts by kind\t{}", st.kind));
            let _ = writeln!(names, "    START {} [{}]", st.kind, st.site);
        }
        let mut by_stage: BTreeMap<i32, Vec<&Setter>> = BTreeMap::new();
        for s in &t.setters {
            if s.quest.get() == Some(q) {
                match s.stage.get() {
                    Some(n) => by_stage.entry(*n).or_default().push(s),
                    None => {
                        bump(
                            &mut c,
                            format!(
                                "{p}\tsetters whose stage the scan does not resolve\t{}",
                                s.kind
                            ),
                        );
                        let _ = writeln!(names, "    OPEN STAGE {} | {}", s.kind, s.site);
                    }
                }
            }
        }
        let held = quest_sets.get(&qk).cloned().unwrap_or_default();
        for (st, v) in &by_stage {
            let h = if held.contains(st) {
                "held"
            } else {
                "not held"
            };
            for s in v {
                bump(
                    &mut c,
                    format!("{p}\tsetters of its stages by kind and need ({h} by the record read)\t{}\t{}", s.kind, s.need.family()),
                );
                let _ = writeln!(
                    names,
                    "    STAGE {st} ({h}) | {} | {} | {}",
                    s.kind,
                    s.need.text(&store),
                    s.site
                );
            }
        }
        let unset = held.iter().filter(|x| !by_stage.contains_key(x)).count();
        bump(
            &mut c,
            format!("{p}\tstages held with no setter in the table\t{unset}"),
        );
    }
    for (n, (q, st)) in chains.iter().enumerate() {
        let p = format!("3 chain {}", n + 1);
        let _ = writeln!(names, "## 3 chain {} from {} stage {st}", n + 1, label(q));
        let mut ends = Counts::new();
        walk_chain(
            &t,
            &store,
            &label,
            q,
            *st,
            0,
            &mut BTreeSet::new(),
            &mut ends,
            &mut names,
        );
        for (k, v) in ends {
            *c.entry(format!("{p}\tpaths back by how they end\t{k}"))
                .or_default() += v;
        }
    }

    // 4 the container change event.
    let p4 = "4 the container change event";
    let event = "oncontainerchanged";
    let handlers: BTreeSet<&String> = lib
        .scripts
        .iter()
        .filter(|(_, s)| {
            s.functions
                .iter()
                .any(|((_, n), f)| n == event && !f.code.is_empty())
        })
        .map(|(n, _)| n)
        .collect();
    *c.entry(format!("{p4}\tscripts defining its handler with code"))
        .or_default() += handlers.len() as u64;
    // How many attachments each handler script serves.
    let mut uses: BTreeMap<String, usize> = BTreeMap::new();
    for x in &atts {
        if let Some(h) = handler_in_chain(&lib, &x.script, event) {
            *uses.entry(h).or_default() += 1;
        }
    }
    for x in &atts {
        let Some(h) = handler_in_chain(&lib, &x.script, event) else {
            continue;
        };
        bump(
            &mut c,
            format!(
                "{p4}\tattachments whose chain handles it, by kind\t{}",
                x.kind()
            ),
        );
        let shared = uses.get(&h).copied().unwrap_or(0) >= 10;
        bump(
            &mut c,
            format!(
                "{p4}\tattachments whose chain handles it, by whether the handler's script is shared by ten attachments or more, and kind\t{shared}\t{}",
                x.kind()
            ),
        );
        bump(
            &mut c,
            format!(
                "{p4}\tattachments whose chain handles it, by the integer properties they store\t{}",
                if x.ints.is_empty() { "none" } else { "some" }
            ),
        );
        let filters = chain_calls(&lib, &x.script, &["addinventoryeventfilter"]);
        bump(
            &mut c,
            format!("{p4}\tattachments whose chain adds an inventory filter\t{filters}"),
        );
        let _ = writeln!(
            names,
            "## 4 {} {} script {} (handler in {h})",
            x.kind(),
            label(&x.record),
            x.script
        );
    }

    // 5 the stages with no setter: the search widened.
    for (n, (q, st)) in stages.iter().enumerate() {
        let p = format!("5 stage {}", n + 1);
        let _ = writeln!(names, "## 5 stage {} is {} stage {st}", n + 1, label(q));
        let exact = t.of_stage(q, *st).len();
        bump(&mut c, format!("{p}\tconstant setters\t{exact}"));
        let flags = t
            .stages
            .get(&record_key(q))
            .and_then(|(_, v)| v.iter().find(|x| i32::from(x.0) == *st))
            .map(|x| format!("0x{:02x}", x.1))
            .unwrap_or("the record does not hold it".into());
        bump(&mut c, format!("{p}\tits stage flags (INDX)\t{flags}"));
        let mut open_stage = 0;
        for s in &t.setters {
            if let (Resolved::Open(w), Some(n)) = (&s.quest, s.stage.get()) {
                if n == st {
                    bump(&mut c, format!("{p}\tsetters of this stage number whose quest the scan does not resolve, by reason\t{w}"));
                    let _ = writeln!(
                        names,
                        "    OPEN QUEST stage {n} | {} | {w} | {}",
                        s.kind, s.site
                    );
                }
            }
            if s.quest.get() == Some(q) && s.stage.get().is_none() {
                open_stage += 1;
            }
        }
        bump(
            &mut c,
            format!(
                "{p}\tsetters of its quest whose stage the scan does not resolve\t{open_stage}"
            ),
        );
        for s in &t.setters {
            if s.quest.get() == Some(q) && s.stage.get().is_none() {
                if let Resolved::Open(w) = &s.stage {
                    bump(
                        &mut c,
                        format!(
                            "{p}\tsetters of its quest whose stage is not resolved\t{}\t{w}",
                            s.kind
                        ),
                    );
                    let _ = writeln!(names, "    OPEN STAGE {} | {} | {}", s.kind, w, s.site);
                }
            }
        }
        let mut props = 0;
        for x in &atts {
            let names_quest: Vec<&String> = x
                .objects
                .iter()
                .filter(|(_, v)| v.contains(q))
                .map(|(k, _)| k)
                .collect();
            if names_quest.is_empty() {
                continue;
            }
            props += 1;
            let sets = chain_calls(&lib, &x.script, &["setstage", "setcurrentstageid"]);
            let int = x.ints.values().any(|v| v.contains(st));
            bump(
                &mut c,
                format!(
                    "{p}\tattachments storing a property that names its quest, by kind; their chain calls a set stage function; an integer property equals the stage\t{}\t{sets}\t{int}",
                    x.kind()
                ),
            );
            if sets || int {
                let _ = writeln!(
                    names,
                    "    PROPERTY {} {} script {} properties {:?} sets {sets} int {int}",
                    x.kind(),
                    label(&x.record),
                    x.script,
                    names_quest
                );
            }
        }
        if props == 0 {
            bump(
                &mut c,
                format!("{p}\tattachments storing a property that names its quest\t0"),
            );
        }
    }
    // Every stage of every winning quest by its INDX flag bits, and whether
    // the table has a setter for it.
    let p5f =
        "5 stages of the winning quests by INDX flag bit; the table has a constant setter for them";
    for (qk, (q, v)) in &t.stages {
        let _ = qk;
        for (n, f) in v {
            let set = !t.of_stage(q, i32::from(*n)).is_empty();
            for bit in [0x01u8, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80] {
                if f & bit != 0 {
                    bump(&mut c, format!("{p5f}\tbit 0x{bit:02x}\t{set}"));
                }
            }
            if *f == 0 {
                bump(&mut c, format!("{p5f}\tno bit\t{set}"));
            }
        }
    }
    let p5 = "5 setters whose quest the scan does not resolve, by reason";
    for s in &t.setters {
        if let Resolved::Open(w) = &s.quest {
            bump(&mut c, format!("{p5}\t{w}"));
        }
    }

    // 6 the lines named.
    if !lines.is_empty() {
        let mut topic_of: BTreeMap<FormKey, FormKey> = BTreeMap::new();
        let mut siblings: BTreeMap<FormKey, Vec<FormKey>> = BTreeMap::new();
        for (o, topic) in store.topic_children() {
            let k = store.overrides().form_key(o.key, store.order());
            siblings.entry(topic.clone()).or_default().push(k.clone());
            topic_of.insert(k, topic);
        }
        for (n, l) in lines.iter().enumerate() {
            let p = format!("6 line {}", n + 1);
            let _ = writeln!(names, "## 6 line {} is {}", n + 1, label(l));
            let Some(topic) = topic_of.get(l).cloned() else {
                bump(&mut c, format!("{p}\tno topic holds it"));
                continue;
            };
            let _ = writeln!(names, "    topic {}", label(&topic));
            let sib = siblings.get(&topic).cloned().unwrap_or_default();
            bump(&mut c, format!("{p}\tlines of its topic\t{}", sib.len()));
            for s in &sib {
                let bb = Budget::new(max);
                let Ok(Some(r)) = store.raw(s, &bb) else {
                    continue;
                };
                let ls = lists(&r, &bb).unwrap_or_default();
                let fs: BTreeSet<u16> = ls
                    .iter()
                    .flat_map(|x| x.conditions.iter().map(|cd| cd.ctda.function))
                    .collect();
                let who = if s == l { "the line" } else { "another line" };
                bump(
                    &mut c,
                    format!(
                        "{p}\tlines of its topic by their condition functions\t{who}\t{}",
                        if fs.is_empty() {
                            "none".to_string()
                        } else {
                            fs.iter()
                                .map(|f| f.to_string())
                                .collect::<Vec<_>>()
                                .join(" ")
                        }
                    ),
                );
                let _ = writeln!(
                    names,
                    "    {who} {} functions {fs:?} tags {:?}",
                    label(s),
                    tags(&r)
                );
            }
            let mut scenes_found: Vec<FormKey> = Vec::new();
            let want = [topic.clone(), l.clone()];
            let ids: BTreeSet<u32> = want.iter().map(|k| k.object_id & 0x00ff_ffff).collect();
            for o in store.winners() {
                if o.deleted
                    || !matches!(
                        &o.signature,
                        b"SCEN" | b"QUST" | b"DIAL" | b"INFO" | b"DLBR" | b"PACK" | b"NPC_"
                    )
                {
                    continue;
                }
                let k = store.overrides().form_key(o.key, store.order());
                let bb = Budget::new(max);
                let Ok(Some((r, plugin))) = store.raw_with_plugin(&k, &bb) else {
                    continue;
                };
                for s in &r.subrecords {
                    let d = r.data(s);
                    if d.len() < 4 {
                        continue;
                    }
                    for off in 0..=d.len() - 4 {
                        let w = u32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]);
                        if !ids.contains(&(w & 0x00ff_ffff)) {
                            continue;
                        }
                        let Ok(Some(f)) = store.order().remap(plugin, w, &r.source.file, s.offset)
                        else {
                            continue;
                        };
                        let Some(which) = want.iter().position(|x| *x == f) else {
                            continue;
                        };
                        if k == want[which] || (which == 0 && sib.contains(&k)) {
                            continue;
                        }
                        let what = if which == 0 { "the topic" } else { "the line" };
                        if r.signature == *b"SCEN" && !scenes_found.contains(&k) {
                            scenes_found.push(k.clone());
                        }
                        bump(
                            &mut c,
                            format!("{p}\twinning records holding a word naming {what}, by type and tag\t{}\t{}", sig(r.signature), sig(s.signature)),
                        );
                        let _ = writeln!(
                            names,
                            "    {what} named by {} in {} at byte {off}",
                            label(&k),
                            sig(s.signature)
                        );
                    }
                }
            }
            for sk in &scenes_found {
                let bb = Budget::new(max);
                let Ok(Some((r, plugin))) = store.raw_with_plugin(sk, &bb) else {
                    continue;
                };
                for l2 in lists(&r, &bb).unwrap_or_default() {
                    for cd in &l2.conditions {
                        let x = &cd.ctda;
                        let ctx = Context {
                            order: store.order(),
                            plugin,
                        };
                        let p1 = match fo4_plugin::condition::form(ctx, x.param1, &r.source.file, 0)
                        {
                            Ok(Some(f)) => sig(store.signature(&f).unwrap_or(*b"????")),
                            _ => format!("the number {}", x.param1),
                        };
                        bump(
                            &mut c,
                            format!(
                                "{p}\tthe scene's conditions: function, operator, comparison, first parameter's type\t{}\t{}\t{:?}\t{p1}",
                                x.function,
                                x.operator(),
                                x.comparison()
                            ),
                        );
                    }
                }
                let ids2 = sk.object_id & 0x00ff_ffff;
                let mut named = 0;
                for o in store.winners() {
                    if o.deleted {
                        continue;
                    }
                    let k = store.overrides().form_key(o.key, store.order());
                    if &k == sk {
                        continue;
                    }
                    let bb = Budget::new(max);
                    let Ok(Some((r, plugin))) = store.raw_with_plugin(&k, &bb) else {
                        continue;
                    };
                    for s in &r.subrecords {
                        let d = r.data(s);
                        if d.len() < 4 {
                            continue;
                        }
                        for off in 0..=d.len() - 4 {
                            let w =
                                u32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]);
                            if w & 0x00ff_ffff != ids2 {
                                continue;
                            }
                            let Ok(Some(f)) =
                                store.order().remap(plugin, w, &r.source.file, s.offset)
                            else {
                                continue;
                            };
                            if &f != sk {
                                continue;
                            }
                            named += 1;
                            bump(&mut c, format!("{p}\twinning records holding a word naming the scene, by type and tag\t{}\t{}", sig(r.signature), sig(s.signature)));
                        }
                    }
                }
                if named == 0 {
                    bump(
                        &mut c,
                        format!("{p}\twinning records holding a word naming the scene\t0"),
                    );
                }
                let _ = writeln!(names, "    scene {} named by {named} words", label(sk));
            }
        }
    }

    // 7 the location change event and location parents.
    let p7 = "7 locations";
    let mut depth_of: BTreeMap<FormKey, Option<FormKey>> = BTreeMap::new();
    for o in store.winners() {
        if o.deleted || o.signature != *b"LCTN" {
            continue;
        }
        let k = store.overrides().form_key(o.key, store.order());
        let parent = fo4_vm::chain::parent_location(&store, max, &k);
        match &parent {
            Ok(Some(p)) => {
                bump(
                    &mut c,
                    format!(
                        "{p7}\twinning location records\twith a parent (PNAM) that is a location"
                    ),
                );
                if store.signature(p) != Some(*b"LCTN") {
                    bump(
                        &mut c,
                        format!("{p7}\tparents that are no winning location"),
                    );
                }
            }
            Ok(None) => bump(
                &mut c,
                format!("{p7}\twinning location records\twithout a parent (PNAM)"),
            ),
            Err(why) => bump(
                &mut c,
                format!("{p7}\twinning location records\tparent not read: {why}"),
            ),
        }
        depth_of.insert(k, parent.ok().flatten());
    }
    for k in depth_of.keys() {
        let mut n = 0;
        let mut at = k.clone();
        while let Some(Some(p)) = depth_of.get(&at) {
            n += 1;
            if n > fo4_vm::chain::MAX_PARENT_LINKS {
                break;
            }
            at = p.clone();
        }
        let b = if n > fo4_vm::chain::MAX_PARENT_LINKS {
            "longer than the run follows".to_string()
        } else {
            n.to_string()
        };
        bump(
            &mut c,
            format!("{p7}\tparent links up to a location without one\t{b}"),
        );
    }
    let calls: usize = lib
        .scripts
        .values()
        .flat_map(|s| s.functions.values())
        .flat_map(|f| f.code.iter())
        .filter(|i| i.op == 23 && matches!(i.args.first(), Some(Lit::Ident(n)) if n == "ischild"))
        .count();
    *c.entry(format!("{p7}\tIsChild call sites")).or_default() += calls as u64;
    let handlers = lib
        .scripts
        .values()
        .filter(|s| {
            s.functions
                .iter()
                .any(|((_, n), f)| n == "onlocationchange" && !f.code.is_empty())
        })
        .count();
    *c.entry(format!(
        "{p7}\tscripts defining the location change handler with code"
    ))
    .or_default() += handlers as u64;
    for x in &atts {
        if handler_in_chain(&lib, &x.script, "onlocationchange").is_some() {
            bump(
                &mut c,
                format!(
                    "{p7}\tattachments whose chain handles the location change, by kind\t{}",
                    x.kind()
                ),
            );
        }
    }

    let mut out = String::from(
        "# chain_census: kinds and counts only (spec/vm.md, The town chain). Sections: 1 winning records with record flag bit 0x4000; 2 constant\n# setters naming a stage the winning quest does not hold; 3 the quests and chains named (numbered); 4 the container change event; 5 the\n# stages named with no setter (numbered); 6 the lines named (numbered); 7 locations: parents and the location change event.\n",
    );
    for (k, v) in &c {
        let _ = writeln!(out, "{v}\t{k}");
    }
    std::fs::write(&a[1], out)?;
    std::fs::write(&a[2], names)?;
    Ok(())
}

/// The setters of a stage and, for each that needs another stage, the
/// setters of that one, up to four steps; each path's end counted by
/// kind.
#[allow(clippy::too_many_arguments)]
fn walk_chain(
    t: &setters::Table,
    store: &Store,
    label: &dyn Fn(&FormKey) -> String,
    q: &FormKey,
    st: i32,
    depth: usize,
    seen: &mut BTreeSet<(String, i32)>,
    ends: &mut Counts,
    names: &mut String,
) {
    let pad = "  ".repeat(depth + 1);
    if !seen.insert((record_key(q), st)) {
        bump(ends, "a loop: a stage already on the path");
        let _ = writeln!(names, "{pad}(loop) {} stage {st}", label(q));
        return;
    }
    let v = t.of_stage(q, st);
    if v.is_empty() {
        let starts = t.of_quest(q).len();
        bump(
            ends,
            format!(
                "a stage with no setter (its quest has {} start(s) in the table)",
                if starts > 0 { "some" } else { "no" }
            ),
        );
        let _ = writeln!(
            names,
            "{pad}{} stage {st}: no setter; starts {starts}",
            label(q)
        );
    }
    for s in v {
        let _ = writeln!(
            names,
            "{pad}{} stage {st} <- {} | {} | {}",
            label(q),
            s.kind,
            s.need.text(store),
            s.site
        );
        match &s.need {
            Need::Stage(q2, s2) if depth < 4 => {
                walk_chain(t, store, label, q2, *s2, depth + 1, seen, ends, names);
            }
            Need::Stage(..) => bump(ends, "four steps back: not followed further"),
            other => bump(
                ends,
                format!("a need other than a stage: {}", other.family()),
            ),
        }
    }
    seen.remove(&(record_key(q), st));
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
