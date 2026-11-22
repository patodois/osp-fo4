#![forbid(unsafe_code)]
//! Census of the story manager nodes and of what could start each quest
//! (`spec/story-manager.md`): `story_census MANIFEST OUT.txt NAMES.txt`.
//!
//! OUT.txt holds counts, record type tags, event codes, function indexes
//! and engine native names only (committed as
//! `spec/story-manager-census.txt`); NAMES.txt names the nodes, quests and
//! scripts behind the counts and is never committed.
//!
//! The script scan is static: every call site in every winning compiled
//! script of a native or scripted function that can start a quest or raise
//! a story event (`Quest.Start`, `Quest.SetCurrentStageID`, the scripted
//! `Quest.setStage` and `Quest.SetAllStages`, `Keyword.SendStoryEvent` and
//! `SendStoryEventAndWait`), with its receiver followed back within the
//! function to `self`, a property (through its variable or a property
//! read on `self`), `GetOwningQuest()` on `self`, a parameter or anything
//! else. A property resolves to the constant object values the VMAD of
//! every record attaching the script (or a script extending it) stores
//! for that property name. What the scan cannot see is counted by kind.
use fo4_core::{Budget, Manifest};
use fo4_pex::Pex;
use fo4_plugin::{
    story::{Kind, Order, Tree},
    typed::Store,
    vmad::{PropertyValue, Script as VScript, Vmad},
    FormKey,
};
use fo4_vfs::Vfs;
use fo4_vm::{quest::QuestInfo, Lit, MapLibrary, Script};
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
fn add(m: &mut Counts, k: impl Into<String>, n: u64) {
    *m.entry(k.into()).or_default() += n;
}
fn code(c: [u8; 4]) -> String {
    c.iter()
        .map(|b| {
            if b.is_ascii_graphic() {
                (*b as char).to_string()
            } else {
                format!("\\x{b:02x}")
            }
        })
        .collect()
}
fn bucket(n: usize) -> &'static str {
    match n {
        0 => "0",
        1 => "1",
        2 => "2",
        3..=4 => "3-4",
        5..=9 => "5-9",
        10..=19 => "10-19",
        20..=49 => "20-49",
        _ => ">=50",
    }
}
fn sig(s: [u8; 4]) -> String {
    String::from_utf8_lossy(&s).into_owned()
}

/// What a record attaching a script is, for the scan's counts.
fn attach_kind(record: [u8; 4], part: &str) -> String {
    match (&record, part) {
        (b"QUST", "own") => "quest script".into(),
        (b"QUST", "fragment") => "quest stage fragment script".into(),
        (b"QUST", "alias") => "quest alias script".into(),
        (b"TERM", "fragment") => "terminal fragment script".into(),
        (b"REFR" | b"ACHR" | b"PGRE" | b"PHZD", _) => format!("placed {} script", sig(record)),
        _ => format!("{} script", sig(record)),
    }
}

/// One attachment of a script to a record with its property values.
struct Attachment {
    record: FormKey,
    kind: String,
    /// The quest the attachment belongs to (a quest's own, fragment or
    /// alias scripts).
    quest: Option<FormKey>,
    /// Lower-case property name to the object values it stores.
    objects: BTreeMap<String, Vec<FormKey>>,
}

fn props(
    s: &VScript<'_>,
    store: &Store,
    plugin: usize,
    file: &str,
) -> BTreeMap<String, Vec<FormKey>> {
    let mut out: BTreeMap<String, Vec<FormKey>> = BTreeMap::new();
    for p in &s.properties {
        let mut keys = Vec::new();
        let mut one = |v: &PropertyValue<'_>| {
            if let PropertyValue::Object(o) = v {
                if o.alias < 0 && o.form_id != 0 {
                    if let Ok(Some(k)) = store.order().remap(plugin, o.form_id, file, 0) {
                        keys.push(k);
                    }
                }
            }
        };
        match &p.value {
            PropertyValue::Array(_, vs) => vs.iter().for_each(&mut one),
            v => one(v),
        }
        out.insert(fo4_pex::index::lower(p.name), keys);
    }
    out
}

/// Where a call's receiver comes from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Recv {
    SelfObj,
    Property(String),
    OwningQuest,
    Parameter,
    Other(&'static str),
}
impl Recv {
    fn kind(&self) -> &'static str {
        match self {
            Recv::SelfObj => "self",
            Recv::Property(_) => "a property",
            Recv::OwningQuest => "GetOwningQuest() on self",
            Recv::Parameter => "a parameter",
            Recv::Other(k) => k,
        }
    }
}

fn ident(l: &Lit) -> Option<&str> {
    match l {
        Lit::Ident(n) => Some(n),
        _ => None,
    }
}

/// The destination slot of the opcodes that write one (as `vm_checks`
/// reads them).
fn dest(op: u8) -> Option<usize> {
    match op {
        1..=19 | 27 | 30 | 31 | 32 | 36 | 37 | 38 => Some(0),
        23 | 25 | 28 => Some(2),
        24 | 34 | 40 => Some(1),
        _ => None,
    }
}

fn receiver(lib: &MapLibrary, s: &Script, f: &fo4_vm::Func, at: usize, l: &Lit) -> Recv {
    let mut name = match ident(l) {
        Some(n) => n.to_string(),
        None => return Recv::Other("a literal"),
    };
    let mut at = at;
    for _ in 0..16 {
        if name == "self" {
            return Recv::SelfObj;
        }
        if f.params.iter().any(|(p, _)| *p == name) {
            return Recv::Parameter;
        }
        // A script variable behind a property of the script or a parent.
        if !f.locals.iter().any(|(p, _)| *p == name) {
            let mut sc = Some(Rc::new(s.clone()));
            let mut guard = 0;
            while let Some(x) = sc {
                if let Some(p) = x
                    .properties
                    .iter()
                    .find(|p| p.variable.as_deref() == Some(&name))
                {
                    return Recv::Property(p.name.clone());
                }
                if x.variable(&name).is_some() {
                    return Recv::Other("a script variable without a property");
                }
                guard += 1;
                sc = if guard > 64 {
                    None
                } else {
                    lib.scripts.get(&x.parent).cloned()
                };
            }
            return Recv::Other("an unknown name");
        }
        // A local: the last instruction before `at` that writes it.
        let write = f.code[..at].iter().enumerate().rev().find(|(_, i)| {
            dest(i.op)
                .and_then(|d| i.args.get(d))
                .and_then(ident)
                .is_some_and(|d| d == name)
        });
        let Some((k, ins)) = write else {
            return Recv::Other("a local not written before the call");
        };
        match ins.op {
            13 | 14 => match ins.args.get(1).and_then(ident) {
                Some(n) => {
                    name = n.to_string();
                    at = k;
                }
                None => return Recv::Other("a literal"),
            },
            28 => {
                let on_self = ins.args.get(1).and_then(ident) == Some("self");
                return match (on_self, ins.args.first().and_then(ident)) {
                    (true, Some(p)) => Recv::Property(p.to_string()),
                    _ => Recv::Other("a property of another object"),
                };
            }
            23 => {
                let n = ins.args.first().and_then(ident).unwrap_or("");
                let on_self = ins.args.get(1).and_then(ident) == Some("self");
                return if n == "getowningquest" && on_self {
                    Recv::OwningQuest
                } else {
                    Recv::Other("the result of a call")
                };
            }
            24 | 25 => return Recv::Other("the result of a call"),
            32 => return Recv::Other("an array element"),
            34 | 40 => return Recv::Other("an array search result"),
            _ => return Recv::Other("the result of another operation"),
        }
    }
    Recv::Other("a chain of copies too long")
}

/// Whether a declared type is or extends `target`.
fn is_a(lib: &MapLibrary, t: &str, target: &str) -> bool {
    let mut at = t.to_string();
    for _ in 0..64 {
        if at == target {
            return true;
        }
        match lib.scripts.get(&at) {
            Some(s) if !s.parent.is_empty() => at = s.parent.clone(),
            _ => return false,
        }
    }
    false
}

/// The declared type of an argument slot in a function.
fn decl_type(lib: &MapLibrary, s: &Script, f: &fo4_vm::Func, l: &Lit) -> Option<String> {
    let n = ident(l)?;
    if n == "self" {
        return Some(s.name.clone());
    }
    if let Some((_, t)) = f.params.iter().chain(&f.locals).find(|(p, _)| p == n) {
        return Some(t.clone());
    }
    let mut at = Some(s.name.clone());
    for _ in 0..64 {
        let x = lib.scripts.get(at.as_ref()?)?;
        if let Some(v) = x.variable(n) {
            return Some(v.type_name.clone());
        }
        at = (!x.parent.is_empty()).then(|| x.parent.clone());
    }
    None
}

/// The natives and scripted functions the scan looks for, by receiver type.
const STARTS: [(&str, &str); 4] = [
    ("quest", "start"),
    ("quest", "setcurrentstageid"),
    ("quest", "setstage"),
    ("quest", "setallstages"),
];
const STORY: [(&str, &str); 2] = [
    ("keyword", "sendstoryevent"),
    ("keyword", "sendstoryeventandwait"),
];

fn run() -> Result<(), E> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 3 {
        return Err("story_census MANIFEST OUT.txt NAMES.txt".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let b = Budget::new(m.limits.max_memory);
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let mut c = Counts::new();
    let mut names = String::new();
    let label = |k: &FormKey| -> String {
        let e = store
            .raw(k, &b.child())
            .ok()
            .flatten()
            .and_then(|r| fo4_vm::bind::editor_id(&r))
            .unwrap_or_default();
        format!("{}:{:06x} {e}", k.plugin, k.object_id)
    };

    // ---- 1. Nodes --------------------------------------------------------
    let tree = Tree::read(&store, &b)?;
    add(&mut c, "1 nodes: read errors", tree.errors.len() as u64);
    for (k, e) in &tree.errors {
        let _ = writeln!(names, "ERROR {} {e}", label(k));
    }
    for n in tree.nodes.values() {
        let s = sig(n.kind.signature());
        bump(&mut c, format!("1 nodes\t{s}"));
        if n.editor_id.is_some() {
            bump(&mut c, format!("1 nodes with EDID\t{s}"));
        }
        let order: Vec<String> = {
            let mut v: Vec<String> = Vec::new();
            for f in &n.fields {
                let t = sig(f.tag);
                let t = match t.as_str() {
                    "CTDA" | "CIS1" | "CIS2" => "condition*".to_string(),
                    "NNAM" | "RNAM" => "(NNAM [RNAM])*".to_string(),
                    _ => t,
                };
                if v.last() != Some(&t) {
                    v.push(t);
                }
            }
            v
        };
        bump(
            &mut c,
            format!("1 subrecord order\t{s}\t{}", order.join(" ")),
        );
        for f in &n.fields {
            let what = if f.unsettled.is_some() {
                "kept as a stored word"
            } else {
                "typed"
            };
            add(
                &mut c,
                format!("1 bytes by tag\t{s}\t{}\t{what}", sig(f.tag)),
                f.size as u64,
            );
        }
        // CITC against the conditions.
        let holds = n.condition_count as usize == n.conditions.len();
        bump(
            &mut c,
            format!(
                "1 CITC equals the number of conditions\t{s}\t{}",
                if holds { "holds" } else { "fails" }
            ),
        );
        let halves = (n.dnam & 0xffff, n.dnam >> 16);
        bump(
            &mut c,
            format!(
                "1 DNAM low u16, high u16\t{s}\t{:#06x}, {:#06x}",
                halves.0, halves.1
            ),
        );
        bump(&mut c, format!("1 XNAM\t{s}\t{}", n.xnam));
        if let Some(v) = n.mnam {
            bump(&mut c, format!("1 MNAM\t{s}\t{v}"));
        }
        if let Some(v) = n.hnam {
            let f = f32::from_bits(v);
            bump(
                &mut c,
                format!(
                    "1 HNAM read as f32\t{s}\t{}",
                    if f.is_finite() {
                        format!("{f}")
                    } else {
                        "not finite".into()
                    }
                ),
            );
        }
        if let Some(q) = n.quest_count {
            bump(
                &mut c,
                format!(
                    "1 QNAM equals the number of NNAM\t{}",
                    if q as usize == n.quests.len() {
                        "holds"
                    } else {
                        "fails"
                    }
                ),
            );
            for e in &n.quests {
                match e.rnam {
                    Some(r) => {
                        let f = f32::from_bits(r);
                        bump(
                            &mut c,
                            format!(
                                "1 RNAM after a quest, read as f32\t{}",
                                if f.is_finite() {
                                    format!("{f}")
                                } else {
                                    "not finite".into()
                                }
                            ),
                        );
                    }
                    None => bump(&mut c, "1 RNAM after a quest\tnone"),
                }
            }
        }
        // Links.
        let target = |r: &fo4_plugin::typed::FormRef| -> String {
            match (&r.key, r.raw) {
                (_, 0) => "zero".into(),
                (_, w) if w & 0x00ff_ffff < 0x800 => {
                    format!("a word below 0x800 naming no record ({w:#x})")
                }
                (Some(k), _) => match store.signature(k) {
                    Some(t) => sig(t),
                    None => "a FormID naming no winning record".into(),
                },
                (None, _) => "a FormID that does not resolve".into(),
            }
        };
        bump(&mut c, format!("2 PNAM names\t{s}\t{}", target(&n.parent)));
        let prev = target(&n.previous);
        let sibling = match (&n.previous.key, &n.parent.key) {
            (Some(p), Some(par)) if n.previous.raw != 0 => {
                let same = tree
                    .nodes
                    .get(p)
                    .is_some_and(|x| x.parent.key.as_ref() == Some(par));
                if same {
                    " (a node of the same parent)"
                } else {
                    " (a node of another parent)"
                }
            }
            _ => "",
        };
        bump(&mut c, format!("2 SNAM names\t{s}\t{prev}{sibling}"));
        for e in &n.quests {
            let t = target(&e.quest);
            bump(&mut c, format!("2 NNAM names\t{t}"));
        }
        // Conditions.
        for cd in &n.conditions {
            bump(
                &mut c,
                format!(
                    "3 condition function index on nodes\t{s}\tfunction {}",
                    cd.ctda.function
                ),
            );
            if cd.ctda.function == 576 {
                let p = cd.ctda.param1.to_le_bytes();
                bump(
                    &mut c,
                    format!(
                        "3 function 576 parameter 1 (u16, two characters)\t{}, {}",
                        u16::from_le_bytes([p[0], p[1]]),
                        String::from_utf8_lossy(&p[2..4])
                    ),
                );
            }
        }
    }
    // Parent orders.
    for (p, (kids, o)) in &tree.children {
        let pk = tree.nodes.get(p).map(|n| sig(n.kind.signature()));
        let pk = pk.unwrap_or_else(|| "not a node".into());
        bump(
            &mut c,
            format!(
                "2 sibling order by previous links\tchildren of {pk}\t{}",
                if *o == Order::Chain {
                    "one chain"
                } else {
                    "not one chain"
                }
            ),
        );
        if *o == Order::Ambiguous {
            let _ = writeln!(
                names,
                "AMBIGUOUS ORDER under {} ({} children)",
                label(p),
                kids.len()
            );
        }
        add(
            &mut c,
            format!("2 children by parent kind\t{pk}"),
            kids.len() as u64,
        );
    }

    // ---- 3. Tree ---------------------------------------------------------
    let mut node_quests: BTreeMap<FormKey, BTreeSet<[u8; 4]>> = BTreeMap::new();
    for (k, n) in &tree.nodes {
        let s = sig(n.kind.signature());
        match tree.path(k) {
            None => bump(
                &mut c,
                format!("3 nodes whose parent links reach no event node\t{s}"),
            ),
            Some(p) => {
                let ev = tree.nodes[*p.last().unwrap_or(&k)].event.unwrap_or([0; 4]);
                bump(
                    &mut c,
                    format!("3 node depth below its event node\t{}", p.len() - 1),
                );
                bump(&mut c, format!("3 nodes by event\t{}\t{s}", code(ev)));
                if n.kind == Kind::Quest {
                    bump(
                        &mut c,
                        format!("3 quests per quest node\t{}", bucket(n.quests.len())),
                    );
                    add(
                        &mut c,
                        format!("3 quest entries by event\t{}", code(ev)),
                        n.quests.len() as u64,
                    );
                    for q in n.quest_keys() {
                        node_quests.entry(q.clone()).or_default().insert(ev);
                    }
                }
            }
        }
    }
    for (ev, qs) in tree.quests_by_event() {
        add(
            &mut c,
            format!("3 distinct quests reachable from the event\t{}", code(ev)),
            qs.len() as u64,
        );
    }
    add(
        &mut c,
        "3 distinct quests listed by any node",
        node_quests.len() as u64,
    );
    for evs in node_quests.values() {
        bump(
            &mut c,
            format!("3 events whose trees list one quest\t{}", evs.len()),
        );
    }

    // ---- 4. Quests -------------------------------------------------------
    let quest_keys: Vec<FormKey> = store
        .winners()
        .filter(|w| !w.deleted && w.signature == *b"QUST")
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .collect();
    let mut flagged = BTreeSet::new();
    let mut enam: BTreeMap<FormKey, Option<[u8; 4]>> = BTreeMap::new();
    for k in &quest_keys {
        let i = QuestInfo::load(&store, k, &b.child())?;
        if i.as_ref().is_some_and(|i| i.start_game_enabled()) {
            flagged.insert(k.clone());
        }
        let r = store.raw(k, &b.child())?;
        let e = r.and_then(|r| {
            r.subrecords
                .iter()
                .find(|s| s.signature == *b"ENAM")
                .and_then(|s| <[u8; 4]>::try_from(r.data(s)).ok())
        });
        enam.insert(k.clone(), e);
    }
    add(&mut c, "4 quests", quest_keys.len() as u64);
    add(
        &mut c,
        "4 quests flagged to start with the game",
        flagged.len() as u64,
    );
    for (k, e) in &enam {
        let listed = node_quests.get(k);
        let what = match (e, listed) {
            (None, None) => "no ENAM, listed by no node".to_string(),
            (None, Some(_)) => "no ENAM, listed by a node".to_string(),
            (Some(e), None) => format!("ENAM {}, listed by no node", code(*e)),
            (Some(e), Some(evs)) if evs.len() == 1 && evs.contains(e) => {
                "ENAM equal to the event of every node listing it".into()
            }
            (Some(_), Some(_)) => "ENAM differs from the event of a node listing it".into(),
        };
        bump(&mut c, format!("4 quest ENAM against the nodes\t{what}"));
        if let Some(e) = e {
            bump(&mut c, format!("4 quests by ENAM\t{}", code(*e)));
        }
    }
    let in_nodes_flagged = node_quests.keys().filter(|k| flagged.contains(*k)).count();
    add(
        &mut c,
        "4 quests listed by a node and flagged to start",
        in_nodes_flagged as u64,
    );

    // ---- 5. Scripts ------------------------------------------------------
    let mut lib = MapLibrary::default();
    {
        let cands = &vfs.candidates;
        for (i, x) in cands.iter().enumerate() {
            let Some(k) = &x.key else { continue };
            if !k.ends_with(b".pex") || cands.get(i + 1).and_then(|n| n.key.as_ref()) == Some(k) {
                continue;
            }
            let bb = Budget::new(m.limits.max_memory);
            let bytes = vfs.read_candidate(x, &bb)?;
            let path = String::from_utf8_lossy(k).into_owned();
            let p = Pex::read(&bytes, &path, &bb)?;
            if let Some(s) = Script::from_pex(&p, 0) {
                lib.add(s);
            }
        }
    }
    add(&mut c, "5 scripts read", lib.scripts.len() as u64);
    // Attachments: every winning record with a VMAD.
    let mut attachments: BTreeMap<String, Vec<Attachment>> = BTreeMap::new();
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
    for (k, s) in &winners {
        let bb = b.child();
        let Some((r, plugin)) = store.raw_with_plugin(k, &bb)? else {
            continue;
        };
        let Some(sub) = r.subrecords.iter().find(|x| x.signature == *b"VMAD") else {
            continue;
        };
        let file = r.source.file.clone();
        let v = match Vmad::read(r.data(sub), r.signature, &file, sub.offset, &bb) {
            Ok(v) => v,
            Err(_) => {
                bump(&mut c, format!("5 VMAD that does not read\t{}", sig(*s)));
                continue;
            }
        };
        let quest = (*s == *b"QUST").then(|| k.clone());
        let mut put = |vs: &VScript<'_>, part: &str, quest: Option<FormKey>| {
            if vs.name.is_empty() {
                return;
            }
            attachments
                .entry(fo4_pex::index::lower(vs.name))
                .or_default()
                .push(Attachment {
                    record: k.clone(),
                    kind: attach_kind(*s, part),
                    quest,
                    objects: props(vs, &store, plugin, &file),
                });
        };
        for x in &v.scripts {
            put(x, "own", quest.clone());
        }
        if let Some(t) = &v.terminal {
            put(&t.script, "fragment", None);
        }
        if let Some(q) = &v.quest {
            put(&q.script, "fragment", quest.clone());
            for al in &q.aliases {
                for x in &al.scripts {
                    put(x, "alias", quest.clone());
                }
            }
        }
        if !v.tail.is_empty() {
            bump(
                &mut c,
                format!(
                    "5 VMAD fragment blocks not read (their script's properties are not seen)\t{}",
                    sig(*s)
                ),
            );
        }
    }
    // Descendants: the attached scripts whose chain includes a script.
    let mut below: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in attachments.keys() {
        let mut at = Some(name.clone());
        let mut guard = 0;
        while let Some(x) = at {
            below.entry(x.clone()).or_default().push(name.clone());
            guard += 1;
            at = lib
                .scripts
                .get(&x)
                .filter(|s| !s.parent.is_empty() && guard < 64)
                .map(|s| s.parent.clone());
        }
    }
    let mut started_by: BTreeMap<FormKey, BTreeSet<String>> = BTreeMap::new();
    let mut story_keywords: BTreeMap<FormKey, u64> = BTreeMap::new();
    let scripts: Vec<Rc<Script>> = lib.scripts.values().cloned().collect();
    for s in &scripts {
        let funcs: Vec<Rc<fo4_vm::Func>> = s
            .functions
            .values()
            .cloned()
            .chain(
                s.properties
                    .iter()
                    .flat_map(|p| p.read.iter().chain(&p.write).cloned()),
            )
            .collect();
        let attached: Vec<&Attachment> = below
            .get(&s.name)
            .into_iter()
            .flatten()
            .flat_map(|n| attachments.get(n).into_iter().flatten())
            .collect();
        for f in &funcs {
            for (at, ins) in f.code.iter().enumerate() {
                if ins.op != 23 {
                    continue;
                }
                let Some(name) = ins.args.first().and_then(ident) else {
                    continue;
                };
                let recv_lit = ins.args.get(1).cloned().unwrap_or(Lit::None);
                let rt = decl_type(&lib, s, f, &recv_lit).unwrap_or_default();
                let start = STARTS.iter().any(|(t, n)| *n == name && is_a(&lib, &rt, t));
                let story = STORY.iter().any(|(t, n)| *n == name && is_a(&lib, &rt, t));
                if !start && !story {
                    continue;
                }
                let native = format!("{}.{name}", if start { "quest" } else { "keyword" });
                let r = receiver(&lib, s, f, at, &recv_lit);
                let attached_text = if attached.is_empty() {
                    "a script no read VMAD attaches".to_string()
                } else {
                    let kinds: BTreeSet<&str> = attached.iter().map(|x| x.kind.as_str()).collect();
                    kinds.into_iter().collect::<Vec<_>>().join(" and ")
                };
                bump(
                    &mut c,
                    format!("5 call sites\t{native}\treceiver {}", r.kind()),
                );
                bump(
                    &mut c,
                    format!("5 call sites by script attachment\t{native}\t{attached_text}"),
                );
                // Constant receivers.
                let mut consts: Vec<(FormKey, String, bool)> = Vec::new();
                for x in &attached {
                    match &r {
                        Recv::Property(p) => {
                            for k in x.objects.get(p).into_iter().flatten() {
                                let own = x.quest.as_ref() == Some(k);
                                consts.push((k.clone(), x.kind.clone(), own));
                            }
                        }
                        Recv::SelfObj | Recv::OwningQuest => {
                            let own = match &r {
                                Recv::SelfObj
                                    if x.kind == "quest script"
                                        || x.kind == "quest stage fragment script" =>
                                {
                                    x.quest.clone()
                                }
                                Recv::OwningQuest => x.quest.clone(),
                                Recv::SelfObj => (store.signature(&x.record) == Some(*b"QUST"))
                                    .then(|| x.record.clone()),
                                _ => None,
                            };
                            if let Some(q) = own {
                                consts.push((q, x.kind.clone(), true));
                            }
                        }
                        _ => {}
                    }
                }
                let typed: Vec<(FormKey, String, bool)> = consts
                    .into_iter()
                    .filter(|(k, _, _)| {
                        let want = if start { *b"QUST" } else { *b"KYWD" };
                        store.signature(k) == Some(want)
                    })
                    .collect();
                bump(
                    &mut c,
                    format!(
                        "5 call sites whose receiver resolves to a constant record\t{native}\t{}",
                        if typed.is_empty() { "no" } else { "yes" }
                    ),
                );
                for (k, kind, own) in &typed {
                    let _ = writeln!(
                        names,
                        "CALL {native} in {}.{} on {} (attached as {kind}{})",
                        s.name,
                        f.name,
                        label(k),
                        if *own { ", its own quest" } else { "" }
                    );
                    if start && *own {
                        bump(
                            &mut c,
                            format!("5 constant receivers that are the calling script's own quest (not a start)\t{native}"),
                        );
                    } else if start {
                        let how = if name == "start" {
                            format!("Start from a {kind}")
                        } else {
                            format!("a stage set from a {kind}")
                        };
                        started_by.entry(k.clone()).or_default().insert(how);
                    } else {
                        *story_keywords.entry(k.clone()).or_default() += 1;
                    }
                }
            }
        }
    }
    add(
        &mut c,
        "5 quests named as the constant receiver of a starting call",
        started_by.len() as u64,
    );
    add(
        &mut c,
        "5 keywords named as the constant receiver of a story event call",
        story_keywords.len() as u64,
    );
    // Script events: which of those keywords a node of the SCPT tree
    // tests with function 576 (selector 0, K1).
    let mut tested: BTreeSet<FormKey> = BTreeSet::new();
    for (k, n) in &tree.nodes {
        if tree.event_of(k) != Some(*b"SCPT") {
            continue;
        }
        for cd in &n.conditions {
            let p = cd.ctda.param1.to_le_bytes();
            if cd.ctda.function == 576 && p == [0, 0, b'K', b'1'] {
                let plugin = store
                    .order()
                    .headers()
                    .iter()
                    .position(|h| h.name.eq_ignore_ascii_case(&k.plugin))
                    .unwrap_or(0);
                if let Ok(Some(kw)) = store.order().remap(plugin, cd.ctda.param2, "", 0) {
                    tested.insert(kw);
                }
            }
        }
    }
    add(
        &mut c,
        "5 keywords a SCPT tree node tests (function 576, selector 0 K1)",
        tested.len() as u64,
    );
    add(
        &mut c,
        "5 keywords both sent by a constant story event call and tested by a SCPT node",
        story_keywords
            .keys()
            .filter(|k| tested.contains(*k))
            .count() as u64,
    );

    // ---- 6. How each quest not flagged could start ------------------------
    for k in &quest_keys {
        if flagged.contains(k) {
            continue;
        }
        let mut ways: BTreeSet<String> = BTreeSet::new();
        if let Some(evs) = node_quests.get(k) {
            for e in evs {
                ways.insert(format!("story event {}", code(*e)));
            }
        }
        if let Some(s) = started_by.get(k) {
            ways.extend(s.iter().cloned());
        }
        let text = if ways.is_empty() {
            "unknown (no node, no constant call found)".to_string()
        } else {
            ways.iter().cloned().collect::<Vec<_>>().join(" + ")
        };
        bump(&mut c, "6 quests not flagged to start");
        // One line per way, and the combination of the two families.
        let node = node_quests.contains_key(k);
        let call = started_by.contains_key(k);
        bump(
            &mut c,
            format!(
                "6 quests not flagged, by family\t{}",
                match (node, call) {
                    (true, true) => "story node and script call",
                    (true, false) => "story node only",
                    (false, true) => "script call only",
                    (false, false) => "unknown",
                }
            ),
        );
        for w in &ways {
            bump(
                &mut c,
                format!("6 quests not flagged that could start by\t{w}"),
            );
        }
        let _ = writeln!(names, "QUEST {}: {text}", label(k));
    }
    for k in &flagged {
        let _ = writeln!(names, "FLAGGED {}", label(k));
    }

    let mut s = String::new();
    writeln!(
        s,
        "# story_census: counts, record type tags, event codes, function indexes and engine native names only (spec/story-manager.md)"
    )?;
    writeln!(
        s,
        "# 1 records and their fields; 2 links; 3 the tree and its conditions; 4 quests; 5 the static scan of compiled scripts; 6 how each quest not flagged to start could start"
    )?;
    for (k, v) in &c {
        writeln!(s, "{v}\t{k}")?;
    }
    std::fs::write(&a[1], s)?;
    std::fs::write(&a[2], names)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
