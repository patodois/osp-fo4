//! Where in the data a quest stage can be set (`spec/vm.md`, Stage
//! setters): a static table over the compiled scripts, the VMAD
//! attachments and the record fields, with what each place needs to happen
//! first. Nothing runs here.
//!
//! Every reading below is a pending rule: that the two set-stage functions
//! set a stage (`quest_set_stage_v1`), that an engine event reaches the
//! script function of its name (`setter_event_entry_v1`), that a fragment
//! runs on its slot (`quest_fragment_entry_v1`, `vmad_fragment_blocks_v1`),
//! that the INFO and SCEN stage fields set the owning quest's stage when
//! the line or the scene or phase begins or ends
//! (`record_set_stage_fields_v1`), and that a startup stage is set when its
//! quest starts (`quest_start_v1`).
use crate::{
    bind,
    program::{Func, Lit, MapLibrary, Script},
    quest::{QuestInfo, STARTUP_STAGE},
};
use fo4_core::{Budget, Result};
use fo4_plugin::{
    story::Tree,
    typed::Store,
    vmad::{PropertyValue, Script as VScript, Vmad, FRAGMENT_BEGIN, FRAGMENT_CHANGE, FRAGMENT_END},
    FormKey,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

/// The functions that set a stage of their quest receiver: the native and
/// the scripted function that calls it (`quest_set_stage_v1`).
pub const SET_STAGE: [&str; 2] = ["setstage", "setcurrentstageid"];
/// The function that starts a quest (`quest_start_v1`).
pub const START: &str = "start";
/// The root type of every quest script.
pub const QUEST: &str = "quest";
/// How many calls of a function that passes its parameter on to a
/// set-stage call are followed outward.
pub const MAX_WRAPPER_DEPTH: usize = 3;
/// How many functions the search for the event that reaches a function
/// visits.
const MAX_VISITS: usize = 256;

/// What part of a VMAD attaches a script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Part {
    /// The record's own script list.
    Own,
    /// A quest alias entry, by alias id.
    Alias(i16),
    /// The record's fragment block.
    Fragment,
}

/// The slot a fragment function runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Slot {
    /// A quest stage fragment: (stage, log entry).
    Stage(u16, i32),
    /// The begin, end or change fragment of an INFO, PACK or SCEN block
    /// (the flag bit).
    Flag(u8),
    /// A scene phase fragment: (phase, flag byte as stored).
    Phase(u32, u8),
    /// A perk entry fragment.
    Perk(u16),
    /// A terminal menu item fragment.
    Terminal(u32),
}

/// One script attached by one VMAD, with its property values.
#[derive(Debug, Clone)]
pub struct Attachment {
    pub record: FormKey,
    pub signature: [u8; 4],
    pub part: Part,
    /// The quest the script belongs to: the quest itself for its own,
    /// fragment and alias scripts; the quest whose group of kind 10 holds
    /// an INFO or SCEN.
    pub quest: Option<FormKey>,
    /// Lower case.
    pub script: String,
    /// Property (lower case) to the record values it stores.
    pub objects: BTreeMap<String, Vec<FormKey>>,
    /// Property to the alias values it stores: (quest, alias id).
    pub aliases: BTreeMap<String, Vec<(FormKey, i16)>>,
    /// Property to the integer values it stores.
    pub ints: BTreeMap<String, Vec<i32>>,
    /// Property to the struct values it stores (one per element of a
    /// struct array): member (lower case) to a record or an integer.
    pub structs: BTreeMap<String, Vec<BTreeMap<String, Member>>>,
    /// Fragment function (lower case) to its slot.
    pub slots: Vec<(String, Slot)>,
}
impl Attachment {
    /// What attaches the script, without names.
    pub fn kind(&self) -> String {
        let s = sig(self.signature);
        match (&self.signature, self.part) {
            (b"QUST", Part::Own) => "a quest's own script".into(),
            (b"QUST", Part::Alias(_)) => "a quest alias script".into(),
            (b"QUST", Part::Fragment) => "a quest stage fragment".into(),
            (b"TERM", Part::Fragment) => "a terminal fragment".into(),
            (b"INFO", Part::Fragment) => "a dialogue line (INFO) fragment".into(),
            (b"SCEN", Part::Fragment) => "a scene fragment".into(),
            (b"PACK", Part::Fragment) => "a package fragment".into(),
            (b"PERK", Part::Fragment) => "a perk fragment".into(),
            (b"REFR" | b"ACHR" | b"PGRE" | b"PHZD", _) => format!("a placed {s}'s own script"),
            (_, Part::Fragment) => format!("a {s} fragment"),
            (b"INFO", _) => "a dialogue line (INFO)'s own script".into(),
            (b"SCEN", _) => "a scene's own script".into(),
            (b"PACK", _) => "a package's own script".into(),
            (b"PERK", _) => "a perk's own script".into(),
            (b"MGEF", _) => "a magic effect's script".into(),
            _ => format!("a base object's script ({s})"),
        }
    }
}

/// A struct member value the table reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Member {
    Record(FormKey),
    Int(i32),
}

fn sig(s: [u8; 4]) -> String {
    String::from_utf8_lossy(&s).into_owned()
}

/// A value the table resolves to a constant, or why not.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Resolved<T> {
    Const(T),
    Open(&'static str),
}
impl<T> Resolved<T> {
    pub fn get(&self) -> Option<&T> {
        match self {
            Resolved::Const(x) => Some(x),
            Resolved::Open(_) => None,
        }
    }
    pub fn why(&self) -> &'static str {
        match self {
            Resolved::Const(_) => "a constant",
            Resolved::Open(w) => w,
        }
    }
}

/// The object a script receiving an engine event is attached to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum On {
    /// A record attaching the script by its own list (a base object, a
    /// placed reference, a quest).
    Record(FormKey, [u8; 4]),
    /// A quest alias: (quest, alias id).
    Alias(FormKey, i16),
    /// No VMAD read attaches the script.
    Unattached,
}

/// The point of a scene a fragment or field runs at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScenePoint {
    Begin,
    End,
    /// A phase's fragment: (phase, flag byte as stored).
    Phase(u32, u8),
    /// A phase's stage field: (phase, false at its begin, true at its end).
    PhaseField(u32, bool),
}

/// What must happen before a setter runs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Need {
    /// A stage of a quest set (its stage fragment runs).
    Stage(FormKey, i32),
    /// An engine event delivered to the script on that object.
    Event(String, On),
    /// A remote event of an engine event name raised by an object of a
    /// type the script registered for: (sender type, event).
    Remote(String, String),
    /// A custom event another script sends.
    Custom,
    /// A scene reaching a point.
    Scene(FormKey, ScenePoint),
    /// A dialogue line begins (false) or ends (true).
    Line(FormKey, bool),
    /// A package's begin, end or change (the flag bit).
    Package(FormKey, u8),
    /// A perk entry fires.
    Perk(FormKey, u16),
    /// The player picks a terminal menu item.
    Terminal(FormKey, u32),
    /// The quest starts (a startup stage).
    QuestStart(FormKey),
    /// No engine event or fragment slot reaches the function within the
    /// script's own chain (called from other scripts, or never).
    Unreached,
}
impl Need {
    /// What the setter needs, without names.
    pub fn text(&self, store: &Store) -> String {
        let on = |o: &On| match o {
            On::Record(_, s) if matches!(s, b"REFR" | b"ACHR" | b"PGRE" | b"PHZD") => {
                format!("a placed {}", sig(*s))
            }
            On::Record(_, [b'Q', b'U', b'S', b'T']) => "the quest".into(),
            On::Record(_, s) => format!("a placement of a base {}", sig(*s)),
            On::Alias(..) => "a quest alias".into(),
            On::Unattached => "an object no VMAD read attaches the script to".into(),
        };
        let _ = store;
        match self {
            Need::Stage(..) => "a stage of a quest set (its stage fragment)".into(),
            Need::Event(e, o) => format!("engine event {e} on {}", on(o)),
            Need::Remote(t, e) => format!("remote event {e} of a {t} the script registered for"),
            Need::Custom => "a custom event another script sends".into(),
            Need::Scene(_, p) => match p {
                ScenePoint::Begin => "a scene begins".into(),
                ScenePoint::End => "a scene ends".into(),
                ScenePoint::Phase(_, f) => format!("a scene phase fragment (flag byte {f})"),
                ScenePoint::PhaseField(_, false) => "a scene phase begins".into(),
                ScenePoint::PhaseField(_, true) => "a scene phase ends".into(),
            },
            Need::Line(_, false) => "a dialogue line begins".into(),
            Need::Line(_, true) => "a dialogue line ends".into(),
            Need::Package(_, b) => match *b {
                FRAGMENT_BEGIN => "a package begins".into(),
                FRAGMENT_END => "a package ends".into(),
                FRAGMENT_CHANGE => "a package changes".into(),
                x => format!("a package fragment of flag {x:#x}"),
            },
            Need::Perk(..) => "a perk entry fires".into(),
            Need::Terminal(..) => "the player picks a terminal menu item".into(),
            Need::QuestStart(_) => "the quest starts".into(),
            Need::Unreached => {
                "a function no engine event or fragment of its script reaches".into()
            }
        }
    }
    /// The family of the world event, for ranking: what produces it.
    pub fn family(&self) -> &'static str {
        match self {
            Need::Stage(..) | Need::QuestStart(_) => "another stage or quest start (a chain)",
            Need::Event(e, _) if PLAYER_EVENTS.contains(&e.as_str()) => {
                "a player action on an object"
            }
            Need::Event(e, _) if e.starts_with("ontimer") => "a timer of the script",
            Need::Event(..) | Need::Remote(..) => "an engine event",
            Need::Custom => "a custom event of a script",
            Need::Scene(..) => "a scene's progress",
            Need::Line(..) => "a dialogue line",
            Need::Package(..) => "an actor's package",
            Need::Perk(..) => "a perk entry",
            Need::Terminal(..) => "a player action on an object",
            Need::Unreached => "not reached from the script's own events",
        }
    }
}

/// The engine events this table counts as a player action on an object
/// (activating it, entering or leaving its trigger).
pub const PLAYER_EVENTS: [&str; 3] = ["onactivate", "ontriggerenter", "ontriggerleave"];

/// One place that can set a stage.
#[derive(Debug, Clone)]
pub struct Setter {
    pub quest: Resolved<FormKey>,
    pub stage: Resolved<i32>,
    /// The kind of place, without names.
    pub kind: String,
    /// What must happen first.
    pub need: Need,
    /// Calls of functions passing a parameter on, between the set-stage
    /// call and this site (0: the call itself).
    pub depth: usize,
    /// Script, function and record behind it; for files never versioned.
    pub site: String,
}

/// One way a quest starts.
#[derive(Debug, Clone)]
pub struct Starter {
    pub quest: Resolved<FormKey>,
    /// The kind of start, without names.
    pub kind: String,
    pub need: Option<Need>,
    pub site: String,
}

/// Every setter and starter, with the stages of every quest.
#[derive(Debug, Default)]
pub struct Table {
    pub setters: Vec<Setter>,
    pub starters: Vec<Starter>,
    /// Every stage the winning quests hold, by quest record key.
    pub stages: BTreeMap<String, (FormKey, Vec<(u16, u8)>)>,
    /// Counts of what the scan met, without names.
    pub counts: BTreeMap<String, u64>,
}
impl Table {
    fn count(&mut self, k: impl Into<String>) {
        *self.counts.entry(k.into()).or_default() += 1;
    }
    /// The setters of one stage (constant quest and stage).
    pub fn of_stage(&self, quest: &FormKey, stage: i32) -> Vec<&Setter> {
        self.setters
            .iter()
            .filter(|s| s.quest.get() == Some(quest) && s.stage.get() == Some(&stage))
            .collect()
    }
    /// The starters of one quest (constant quest).
    pub fn of_quest(&self, quest: &FormKey) -> Vec<&Starter> {
        self.starters
            .iter()
            .filter(|s| s.quest.get() == Some(quest))
            .collect()
    }
}

// ---- Reading the attachments -------------------------------------------

/// Every script a winning VMAD attaches, with its property values and its
/// fragment slots. `owners` gives the quest whose group of kind 10 holds
/// an INFO or SCEN (by record key).
pub fn attachments(
    store: &Store,
    owners: &BTreeMap<String, FormKey>,
    b: &Budget,
) -> Result<(Vec<Attachment>, BTreeMap<String, u64>)> {
    let mut out = Vec::new();
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
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
                *counts
                    .entry(format!("VMAD that does not read\t{}", sig(*s)))
                    .or_default() += 1;
                continue;
            }
        };
        let quest = if *s == *b"QUST" {
            Some(k.clone())
        } else {
            owners.get(&bind::record_key(k)).cloned()
        };
        let mut put = |vs: &VScript<'_>, part: Part, slots: Vec<(String, Slot)>| {
            if vs.name.is_empty() {
                return;
            }
            let (objects, aliases, ints, structs) = values(vs, store, plugin, &file);
            out.push(Attachment {
                record: k.clone(),
                signature: *s,
                part,
                quest: quest.clone(),
                script: fo4_pex::index::lower(vs.name),
                objects,
                aliases,
                ints,
                structs,
                slots,
            });
        };
        let lower = |f: &[u8]| fo4_pex::index::lower(f);
        for x in &v.scripts {
            put(x, Part::Own, Vec::new());
        }
        if let Some(t) = &v.terminal {
            let slots = t
                .fragments
                .iter()
                .map(|f| (lower(f.function), Slot::Terminal(f.index)))
                .collect();
            put(&t.script, Part::Fragment, slots);
        }
        if let Some(q) = &v.quest {
            let slots = q
                .fragments
                .iter()
                .map(|f| (lower(f.function), Slot::Stage(f.stage, f.log_entry)))
                .collect();
            put(&q.script, Part::Fragment, slots);
            for al in &q.aliases {
                for x in &al.scripts {
                    put(x, Part::Alias(al.object.alias), Vec::new());
                }
            }
        }
        for block in [v.info.as_ref(), v.package.as_ref()].into_iter().flatten() {
            let slots = block
                .fragments
                .iter()
                .map(|(bit, f)| (lower(f.function), Slot::Flag(*bit)))
                .collect();
            put(&block.script, Part::Fragment, slots);
        }
        if let Some(sc) = &v.scene {
            let mut slots: Vec<(String, Slot)> = sc
                .block
                .fragments
                .iter()
                .map(|(bit, f)| (lower(f.function), Slot::Flag(*bit)))
                .collect();
            slots.extend(
                sc.phases
                    .iter()
                    .map(|p| (lower(p.fragment.function), Slot::Phase(p.phase, p.flags))),
            );
            put(&sc.block.script, Part::Fragment, slots);
        }
        if let Some(p) = &v.perk {
            let slots = p
                .fragments
                .iter()
                .map(|f| (lower(f.fragment.function), Slot::Perk(f.entry)))
                .collect();
            put(&p.script, Part::Fragment, slots);
        }
        if !v.tail.is_empty() {
            *counts
                .entry(format!("VMAD with bytes after its blocks\t{}", sig(*s)))
                .or_default() += 1;
        }
    }
    Ok((out, counts))
}

type Values = (
    BTreeMap<String, Vec<FormKey>>,
    BTreeMap<String, Vec<(FormKey, i16)>>,
    BTreeMap<String, Vec<i32>>,
    BTreeMap<String, Vec<BTreeMap<String, Member>>>,
);

fn values(s: &VScript<'_>, store: &Store, plugin: usize, file: &str) -> Values {
    let (mut objects, mut aliases, mut ints, mut structs): Values = Default::default();
    let member = |ms: &[fo4_plugin::vmad::Property<'_>]| -> BTreeMap<String, Member> {
        let mut out = BTreeMap::new();
        for m in ms {
            let v = match &m.value {
                PropertyValue::Object(o) if o.form_id != 0 && o.alias < 0 => store
                    .order()
                    .remap(plugin, o.form_id, file, 0)
                    .ok()
                    .flatten()
                    .map(Member::Record),
                PropertyValue::Int(n) => Some(Member::Int(*n)),
                _ => None,
            };
            if let Some(v) = v {
                out.insert(fo4_pex::index::lower(m.name), v);
            }
        }
        out
    };
    for p in &s.properties {
        let name = fo4_pex::index::lower(p.name);
        match &p.value {
            PropertyValue::Struct(ms) => {
                structs.entry(name.clone()).or_default().push(member(ms));
            }
            PropertyValue::StructArray(items) => {
                for ms in items {
                    structs.entry(name.clone()).or_default().push(member(ms));
                }
            }
            _ => {}
        }
        let mut one = |v: &PropertyValue<'_>| match v {
            PropertyValue::Object(o) if o.form_id != 0 => {
                if let Ok(Some(k)) = store.order().remap(plugin, o.form_id, file, 0) {
                    if o.alias >= 0 {
                        aliases.entry(name.clone()).or_default().push((k, o.alias));
                    } else {
                        objects.entry(name.clone()).or_default().push(k);
                    }
                }
            }
            PropertyValue::Int(n) => ints.entry(name.clone()).or_default().push(*n),
            _ => {}
        };
        match &p.value {
            PropertyValue::Array(_, vs) => vs.iter().for_each(&mut one),
            v => one(v),
        }
    }
    (objects, aliases, ints, structs)
}

// ---- The static scan of the compiled scripts ----------------------------

/// Where an argument of a call comes from, within its function.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Src {
    SelfObj,
    /// A property of the script (by property name).
    Prop(String),
    /// `GetOwningQuest()` on `self`.
    OwningQuest,
    /// `GetOwningQuest()` on a property of the script.
    OwningQuestOf(String),
    /// A parameter of the function, by position.
    Param(usize),
    /// An element of an array property of the script (any element).
    Elem(String),
    /// A member of a struct (by member name).
    Member(Box<Src>, String),
    Int(i32),
    Other(&'static str),
}
impl Src {
    fn has_param(&self) -> bool {
        match self {
            Src::Param(_) => true,
            Src::Member(b, _) => b.has_param(),
            _ => false,
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

/// The scripts of a chain, most derived first.
fn chain(lib: &MapLibrary, name: &str) -> Vec<Rc<Script>> {
    let mut out = Vec::new();
    let mut at = name.to_string();
    for _ in 0..crate::MAX_PARENTS {
        let Some(s) = lib.scripts.get(&at) else { break };
        out.push(s.clone());
        if s.parent.is_empty() {
            break;
        }
        at = s.parent.clone();
    }
    out
}

/// Whether a type is or extends `target`.
fn is_a(lib: &MapLibrary, t: &str, target: &str) -> bool {
    chain(lib, t).iter().any(|s| s.name == target)
}

/// The declared type of an argument slot in a function.
fn decl_type(lib: &MapLibrary, f: &Func, l: &Lit) -> Option<String> {
    let n = ident(l)?;
    if n == "self" {
        return Some(f.script.clone());
    }
    if let Some((_, t)) = f.params.iter().chain(&f.locals).find(|(p, _)| p == n) {
        return Some(t.clone());
    }
    chain(lib, &f.script)
        .iter()
        .find_map(|s| s.variable(n).map(|v| v.type_name.clone()))
}

/// Follows an argument back within its function to where it comes from.
fn trace(lib: &MapLibrary, f: &Func, at: usize, l: &Lit) -> Src {
    let mut name = match l {
        Lit::Int(n) => return Src::Int(*n),
        Lit::Ident(n) => n.clone(),
        Lit::None => return Src::Other("none"),
        _ => return Src::Other("a literal of another type"),
    };
    let mut at = at;
    for _ in 0..16 {
        if name == "self" {
            return Src::SelfObj;
        }
        if let Some(i) = f.params.iter().position(|(p, _)| *p == name) {
            return Src::Param(i);
        }
        if !f.locals.iter().any(|(p, _)| *p == name) {
            for s in chain(lib, &f.script) {
                if let Some(p) = s
                    .properties
                    .iter()
                    .find(|p| p.variable.as_deref() == Some(&name))
                {
                    return Src::Prop(p.name.clone());
                }
                if s.variable(&name).is_some() {
                    return Src::Other("a script variable without a property");
                }
            }
            return Src::Other("an unknown name");
        }
        // A local: the last instruction before `at` that writes it.
        let write = f.code[..at].iter().enumerate().rev().find(|(_, i)| {
            dest(i.op)
                .and_then(|d| i.args.get(d))
                .and_then(ident)
                .is_some_and(|d| d == name)
        });
        let Some((k, ins)) = write else {
            return Src::Other("a local not written before the call");
        };
        match ins.op {
            13 | 14 => match ins.args.get(1) {
                Some(Lit::Int(n)) => return Src::Int(*n),
                Some(Lit::Ident(n)) => {
                    name = n.clone();
                    at = k;
                }
                _ => return Src::Other("a literal of another type"),
            },
            28 => {
                let on_self = ins.args.get(1).and_then(ident) == Some("self");
                return match (on_self, ins.args.first().and_then(ident)) {
                    (true, Some(p)) => Src::Prop(p.to_string()),
                    _ => Src::Other("a property of another object"),
                };
            }
            23 => {
                let n = ins.args.first().and_then(ident).unwrap_or("");
                if n != "getowningquest" {
                    return Src::Other("the result of a call");
                }
                let recv = ins.args.get(1).cloned().unwrap_or(Lit::None);
                return match trace(lib, f, k, &recv) {
                    Src::SelfObj => Src::OwningQuest,
                    Src::Prop(p) => Src::OwningQuestOf(p),
                    _ => Src::Other("the owning quest of another object"),
                };
            }
            24 | 25 => return Src::Other("the result of a call"),
            32 => {
                let arr = ins.args.get(1).cloned().unwrap_or(Lit::None);
                return match trace(lib, f, k, &arr) {
                    Src::Prop(p) => Src::Elem(p),
                    _ => Src::Other("an array element"),
                };
            }
            38 => {
                let base = ins.args.get(1).cloned().unwrap_or(Lit::None);
                let Some(m) = ins.args.get(2).and_then(ident) else {
                    return Src::Other("a struct member of an unknown name");
                };
                return match trace(lib, f, k, &base) {
                    b @ (Src::Param(_) | Src::Prop(_) | Src::Elem(_)) => {
                        Src::Member(Box::new(b), m.to_string())
                    }
                    _ => Src::Other("a member of a struct the scan does not follow"),
                };
            }
            34 | 40 => return Src::Other("an array search result"),
            _ => return Src::Other("the result of another operation"),
        }
    }
    Src::Other("a chain of copies too long")
}

/// One call that sets a stage (or starts a quest), with where its quest
/// and stage come from, in the function that holds it.
#[derive(Debug, Clone)]
struct Site {
    func: Rc<Func>,
    quest: Src,
    stage: Src,
    depth: usize,
    start: bool,
}

/// The engine events: functions without instructions whose name starts
/// with `on`, declared by a script that declares a native function (the
/// engine's own types; `setter_event_entry_v1`).
pub fn engine_events(lib: &MapLibrary) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for s in lib.scripts.values() {
        if !s.functions.values().any(|f| f.native()) {
            continue;
        }
        for f in s.functions.values() {
            if !f.native() && f.code.is_empty() && f.name.starts_with("on") {
                out.insert(f.name.clone());
            }
        }
    }
    out
}

/// What enters a function: an engine event, a remote or custom event, a
/// fragment slot, or nothing found.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Entry {
    Event(String),
    Remote(String, String),
    Custom,
    Slot(Slot),
}

struct Scan<'l> {
    lib: &'l MapLibrary,
    events: BTreeSet<String>,
    /// (attached script, function) to the entries that reach it.
    entries: BTreeMap<(String, String), BTreeSet<Entry>>,
}
impl Scan<'_> {
    fn remote(&self, name: &str) -> Option<Entry> {
        let rest = name.strip_prefix("::remote_")?;
        for (i, _) in rest.match_indices('_') {
            let (t, e) = (&rest[..i], &rest[i + 1..]);
            if self.events.contains(e) {
                if let Some(s) = self.lib.scripts.get(t) {
                    // An engine type by name; a scripts' own type is not
                    // named (it would name a script).
                    let engine = s.functions.values().any(|f| f.native());
                    let t = if engine { t } else { "script type" };
                    return Some(Entry::Remote(t.to_string(), e.to_string()));
                }
            }
        }
        Some(Entry::Custom)
    }
    /// The entries that reach a function on an instance of `attached`
    /// (calls on `self` and parent calls within the chain), the fragment
    /// slots of the attachment counted as entries.
    fn entries(&mut self, attached: &str, func: &str, slots: &[(String, Slot)]) -> BTreeSet<Entry> {
        let key = (attached.to_string(), func.to_string());
        if slots.is_empty() {
            if let Some(e) = self.entries.get(&key) {
                return e.clone();
            }
        }
        let funcs: Vec<Rc<Func>> = chain(self.lib, attached)
            .iter()
            .flat_map(|s| {
                s.functions.values().cloned().chain(
                    s.properties
                        .iter()
                        .flat_map(|p| p.read.iter().chain(&p.write).cloned()),
                )
            })
            .collect();
        let mut out = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut todo = vec![func.to_string()];
        while let Some(n) = todo.pop() {
            if !seen.insert(n.clone()) || seen.len() > MAX_VISITS {
                continue;
            }
            if let Some((_, s)) = slots.iter().find(|(f, _)| *f == n) {
                out.insert(Entry::Slot(*s));
                continue;
            }
            if self.events.contains(&n) {
                out.insert(Entry::Event(n));
                continue;
            }
            if let Some(e) = self.remote(&n) {
                out.insert(e);
                continue;
            }
            for f in &funcs {
                let calls = f.code.iter().any(|i| match i.op {
                    23 => {
                        i.args.first().and_then(ident) == Some(n.as_str())
                            && i.args.get(1).and_then(ident) == Some("self")
                    }
                    24 => i.args.first().and_then(ident) == Some(n.as_str()),
                    _ => false,
                });
                if calls {
                    todo.push(f.name.clone());
                }
            }
        }
        if slots.is_empty() {
            self.entries.insert(key, out.clone());
        }
        out
    }
}

fn functions(s: &Script) -> Vec<Rc<Func>> {
    s.functions
        .values()
        .cloned()
        .chain(
            s.properties
                .iter()
                .flat_map(|p| p.read.iter().chain(&p.write).cloned()),
        )
        .collect()
}

/// The direct set-stage and start calls of every script, and the calls of
/// the functions that pass a parameter on to one, up to
/// [`MAX_WRAPPER_DEPTH`].
fn sites(lib: &MapLibrary, counts: &mut BTreeMap<String, u64>) -> Vec<Site> {
    let mut out = Vec::new();
    for s in lib.scripts.values() {
        for f in functions(s) {
            for (at, ins) in f.code.iter().enumerate() {
                if ins.op != 23 {
                    continue;
                }
                let Some(name) = ins.args.first().and_then(ident) else {
                    continue;
                };
                let set = SET_STAGE.contains(&name);
                if !set && name != START {
                    continue;
                }
                let recv = ins.args.get(1).cloned().unwrap_or(Lit::None);
                let rt = decl_type(lib, &f, &recv).unwrap_or_default();
                if !is_a(lib, &rt, QUEST) {
                    continue;
                }
                let quest = trace(lib, &f, at, &recv);
                // The definitions on the quest type itself: only the
                // functions taking the quest and stage in a struct
                // parameter are followed to their callers.
                if f.script == QUEST {
                    if !matches!(&quest, Src::Member(b, _) if b.has_param()) {
                        continue;
                    }
                    *counts
                        .entry(
                            "functions of the quest type taking the quest and stage in a struct"
                                .into(),
                        )
                        .or_default() += 1;
                }
                let stage = if set {
                    match ins.extra.first() {
                        Some(a) => trace(lib, &f, at, a),
                        None => Src::Other("no argument"),
                    }
                } else {
                    Src::Other("not a stage")
                };
                *counts
                    .entry(format!(
                        "call sites\t{}\t{}",
                        if set { "set stage" } else { "quest start" },
                        if name == "setcurrentstageid" {
                            "the native"
                        } else if set {
                            "the scripted function"
                        } else {
                            "the native"
                        }
                    ))
                    .or_default() += 1;
                out.push(Site {
                    func: f.clone(),
                    quest,
                    stage,
                    depth: 0,
                    start: !set,
                });
            }
        }
    }
    // Functions that pass a parameter on, followed outward. Every call
    // site by the function name it calls, once.
    let mut calls: BTreeMap<String, Vec<(Rc<Func>, usize)>> = BTreeMap::new();
    for s in lib.scripts.values() {
        for f in functions(s) {
            for (at, ins) in f.code.iter().enumerate() {
                let name = match ins.op {
                    23 | 24 => ins.args.first().and_then(ident),
                    25 => ins.args.get(1).and_then(ident),
                    _ => None,
                };
                if let Some(n) = name {
                    calls
                        .entry(n.to_string())
                        .or_default()
                        .push((f.clone(), at));
                }
            }
        }
    }
    let mut frontier: Vec<Site> = out
        .iter()
        .filter(|x| x.quest.has_param() || x.stage.has_param())
        .cloned()
        .collect();
    for depth in 1..=MAX_WRAPPER_DEPTH {
        let mut next = Vec::new();
        for w in &frontier {
            for (f, at) in calls.get(&w.func.name).into_iter().flatten() {
                let (f, at) = (f.clone(), *at);
                let ins = &f.code[at];
                let (recv, args) = match ins.op {
                    23 => (ins.args.get(1).cloned(), &ins.extra),
                    24 => (None, &ins.extra),
                    _ => {
                        if ins.args.first().and_then(ident) != Some(w.func.script.as_str()) {
                            continue;
                        }
                        (None, &ins.extra)
                    }
                };
                // The receiver must be the wrapper's script or one extending
                // it.
                let on_self = match (&recv, ins.op) {
                    (Some(r), _) => {
                        let t = decl_type(lib, &f, r).unwrap_or_default();
                        if !is_a(lib, &t, &w.func.script) {
                            continue;
                        }
                        ident(r) == Some("self")
                    }
                    (None, 24) => {
                        if !is_a(lib, &f.script, &w.func.script) || f.script == w.func.script {
                            continue;
                        }
                        true
                    }
                    _ => {
                        if !w.func.global() {
                            continue;
                        }
                        false
                    }
                };
                fn map(
                    src: &Src,
                    lib: &MapLibrary,
                    f: &Func,
                    at: usize,
                    args: &[Lit],
                    recv: &Option<Lit>,
                    on_self: bool,
                ) -> Src {
                    match src {
                        Src::Member(b, m) => match map(b, lib, f, at, args, recv, on_self) {
                            x @ (Src::Param(_) | Src::Prop(_) | Src::Elem(_)) => {
                                Src::Member(Box::new(x), m.clone())
                            }
                            Src::Other(w) => Src::Other(w),
                            _ => Src::Other("a member of a struct the scan does not follow"),
                        },
                        Src::Elem(_) if on_self => src.clone(),
                        Src::Elem(_) => Src::Other("a property of the called object"),
                        Src::Param(i) => match args.get(*i) {
                            Some(a) => trace(lib, f, at, a),
                            None => Src::Other("a parameter the call does not pass"),
                        },
                        Src::SelfObj => match recv {
                            Some(r) => trace(lib, f, at, r),
                            None if on_self => Src::SelfObj,
                            None => Src::Other("self of a global function"),
                        },
                        Src::Prop(_) | Src::OwningQuest | Src::OwningQuestOf(_) if on_self => {
                            src.clone()
                        }
                        Src::Prop(_) => Src::Other("a property of the called object"),
                        Src::OwningQuest | Src::OwningQuestOf(_) => {
                            Src::Other("the owning quest of the called object")
                        }
                        x => x.clone(),
                    }
                }
                next.push(Site {
                    func: f.clone(),
                    quest: map(&w.quest, lib, &f, at, args, &recv, on_self),
                    stage: map(&w.stage, lib, &f, at, args, &recv, on_self),
                    depth,
                    start: w.start,
                });
            }
        }
        *counts
            .entry(format!(
                "calls of functions passing a parameter on\tdepth {depth}"
            ))
            .or_default() += next.len() as u64;
        frontier = next
            .iter()
            .filter(|x| x.quest.has_param() || x.stage.has_param())
            .cloned()
            .collect();
        out.extend(next);
    }
    out
}

/// Builds the table: the static scan of `lib` over `atts`, the record
/// fields that set a stage, the startup stages and how each quest can
/// start (`tree`: the story manager's nodes).
pub fn table(
    lib: &MapLibrary,
    atts: &[Attachment],
    store: &Store,
    tree: Option<&Tree>,
    b: &Budget,
) -> Result<Table> {
    let mut t = Table::default();
    let mut counts = BTreeMap::new();
    let all = sites(lib, &mut counts);
    t.counts = counts;
    let mut scan = Scan {
        lib,
        events: engine_events(lib),
        entries: BTreeMap::new(),
    };
    // Attachments by script, and the attached scripts below each script.
    let mut by_script: BTreeMap<&str, Vec<&Attachment>> = BTreeMap::new();
    for a in atts {
        by_script.entry(a.script.as_str()).or_default().push(a);
    }
    let mut below: BTreeMap<String, Vec<&Attachment>> = BTreeMap::new();
    for (name, list) in &by_script {
        for s in chain(lib, name) {
            below.entry(s.name.clone()).or_default().extend(list.iter());
        }
        if !lib.scripts.contains_key(*name) {
            below
                .entry(name.to_string())
                .or_default()
                .extend(list.iter());
        }
    }
    for site in &all {
        // A site whose quest or stage is still a parameter is counted by
        // its callers.
        if site.quest.has_param() || site.stage.has_param() {
            t.count(if site.depth >= MAX_WRAPPER_DEPTH {
                "sites left as a parameter after the deepest call followed"
            } else {
                "sites resolved at their callers (a parameter)"
            });
            if site.depth < MAX_WRAPPER_DEPTH {
                continue;
            }
        }
        let host = site.func.script.clone();
        let list: Vec<&Attachment> = below.get(&host).cloned().unwrap_or_default();
        if list.is_empty() {
            let entries = scan.entries(&host, &site.func.name, &[]);
            let quest = resolve_quest(&site.quest, None);
            let pairs = resolve_stage(lib, &site.func, &site.stage, None)
                .into_iter()
                .map(|s| (quest.clone(), s))
                .collect();
            emit(
                &mut t,
                site,
                pairs,
                "a script no read VMAD attaches".into(),
                &entries,
                None,
            );
            continue;
        }
        for a in list {
            let entries = scan.entries(&a.script, &site.func.name, &a.slots);
            let pairs = pairs(lib, site, a);
            emit(&mut t, site, pairs, a.kind(), &entries, Some(a));
        }
    }
    fields(&mut t, store, b)?;
    starts(&mut t, store, tree, b)?;
    // One entry per distinct setter: a function reached by several paths
    // or calling the same function twice gives the same one.
    let mut seen = BTreeSet::new();
    t.setters.retain(|x| {
        seen.insert(format!(
            "{:?}",
            (&x.quest, &x.stage, &x.kind, &x.need, x.depth, &x.site)
        ))
    });
    let mut seen = BTreeSet::new();
    t.starters
        .retain(|x| seen.insert(format!("{:?}", (&x.quest, &x.kind, &x.need, &x.site))));
    Ok(t)
}

/// The (quest, stage) pairs a site gives on one attachment: the members of
/// one struct value together when both come from the same struct
/// property, every combination otherwise.
fn pairs(lib: &MapLibrary, site: &Site, a: &Attachment) -> Vec<(Resolved<FormKey>, Resolved<i32>)> {
    if let (Src::Member(bq, mq), Src::Member(bs, ms)) = (&site.quest, &site.stage) {
        if bq == bs {
            if let Src::Prop(p) | Src::Elem(p) = &**bq {
                let Some(list) = a.structs.get(p).filter(|l| !l.is_empty()) else {
                    return vec![(
                        Resolved::Open("a struct property without a stored value"),
                        Resolved::Open("a struct property without a stored value"),
                    )];
                };
                return list
                    .iter()
                    .map(|m| {
                        let q = match m.get(mq) {
                            Some(Member::Record(k)) => Resolved::Const(k.clone()),
                            _ => Resolved::Open("a struct value without that member"),
                        };
                        let s = match m.get(ms) {
                            Some(Member::Int(n)) => Resolved::Const(*n),
                            _ => Resolved::Open("a struct value without that member"),
                        };
                        (q, s)
                    })
                    .collect();
            }
        }
    }
    let quests = expand_quest(&site.quest, a);
    let stages = resolve_stage(lib, &site.func, &site.stage, Some(a));
    let mut out = Vec::new();
    for q in &quests {
        for s in &stages {
            out.push((q.clone(), s.clone()));
        }
    }
    out
}

fn resolve_quest(src: &Src, a: Option<&Attachment>) -> Resolved<FormKey> {
    expand_quest_opt(src, a)
        .into_iter()
        .next()
        .unwrap_or(Resolved::Open("no value"))
}
fn expand_quest(src: &Src, a: &Attachment) -> Vec<Resolved<FormKey>> {
    expand_quest_opt(src, Some(a))
}
fn expand_quest_opt(src: &Src, a: Option<&Attachment>) -> Vec<Resolved<FormKey>> {
    let open = |w| vec![Resolved::Open(w)];
    match (src, a) {
        (Src::SelfObj, Some(a)) => {
            if a.signature == *b"QUST" && matches!(a.part, Part::Own | Part::Fragment) {
                vec![Resolved::Const(a.record.clone())]
            } else {
                open("self is not a quest")
            }
        }
        (Src::OwningQuest, Some(a)) => match &a.quest {
            Some(q) => vec![Resolved::Const(q.clone())],
            None => open("the owning quest of the record is not read"),
        },
        (Src::OwningQuestOf(p), Some(a)) => match a.aliases.get(p) {
            Some(v) if !v.is_empty() => v.iter().map(|x| Resolved::Const(x.0.clone())).collect(),
            _ => open("a property without an alias value"),
        },
        (Src::Prop(p), Some(a)) => match a.objects.get(p) {
            Some(v) if !v.is_empty() => v.iter().map(|k| Resolved::Const(k.clone())).collect(),
            _ => open("a property without a record value"),
        },
        (Src::Elem(p), Some(a)) => match a.objects.get(p) {
            Some(v) if !v.is_empty() => v.iter().map(|k| Resolved::Const(k.clone())).collect(),
            _ => open("an array property without a record value"),
        },
        (Src::Member(base, m), Some(a)) => match &**base {
            Src::Prop(p) | Src::Elem(p) => {
                let v: Vec<Resolved<FormKey>> = a
                    .structs
                    .get(p)
                    .into_iter()
                    .flatten()
                    .filter_map(|x| match x.get(m) {
                        Some(Member::Record(k)) => Some(Resolved::Const(k.clone())),
                        _ => None,
                    })
                    .collect();
                if v.is_empty() {
                    open("a struct property without a record member")
                } else {
                    v
                }
            }
            _ => open("a member of a struct the scan does not follow"),
        },
        (Src::Param(_), _) => open("a parameter of a function no call resolves"),
        (Src::Other(w), _) => open(w),
        (Src::Int(_), _) => open("a number"),
        (_, None) => open("no attachment gives the value"),
    }
}

fn resolve_stage(
    lib: &MapLibrary,
    f: &Func,
    src: &Src,
    a: Option<&Attachment>,
) -> Vec<Resolved<i32>> {
    match src {
        Src::Int(n) => vec![Resolved::Const(*n)],
        Src::Prop(p) => {
            if let Some(v) = a.and_then(|a| a.ints.get(p)).filter(|v| !v.is_empty()) {
                return v.iter().map(|n| Resolved::Const(*n)).collect();
            }
            // The property's variable's stored initial value
            // (`vmad_property_sets_variable_v1`, `pex_default_values_v1`).
            for s in chain(lib, &f.script) {
                if let Some(pr) = s.property(p) {
                    let init = pr
                        .variable
                        .as_ref()
                        .and_then(|v| s.variable(v))
                        .map(|v| v.init.clone());
                    return match init {
                        Some(Lit::Int(n)) => vec![Resolved::Const(n)],
                        Some(Lit::None) => vec![Resolved::Const(0)],
                        _ => vec![Resolved::Open("a property whose value is not a number")],
                    };
                }
            }
            vec![Resolved::Open("a property not declared")]
        }
        Src::Param(_) => vec![Resolved::Open("a parameter of a function no call resolves")],
        Src::Elem(p) => match a.and_then(|a| a.ints.get(p)).filter(|v| !v.is_empty()) {
            Some(v) => v.iter().map(|n| Resolved::Const(*n)).collect(),
            None => vec![Resolved::Open("an array property without a number value")],
        },
        Src::Member(base, m) => match (&**base, a) {
            (Src::Prop(p) | Src::Elem(p), Some(a)) => {
                let v: Vec<Resolved<i32>> = a
                    .structs
                    .get(p)
                    .into_iter()
                    .flatten()
                    .filter_map(|x| match x.get(m) {
                        Some(Member::Int(n)) => Some(Resolved::Const(*n)),
                        _ => None,
                    })
                    .collect();
                if v.is_empty() {
                    vec![Resolved::Open("a struct property without a number member")]
                } else {
                    v
                }
            }
            _ => vec![Resolved::Open(
                "a member of a struct the scan does not follow",
            )],
        },
        Src::Other("not a stage") => vec![Resolved::Open("not a stage")],
        Src::Other(w) => vec![Resolved::Open(w)],
        _ => vec![Resolved::Open("not a number")],
    }
}

fn emit(
    t: &mut Table,
    site: &Site,
    pairs: Vec<(Resolved<FormKey>, Resolved<i32>)>,
    attach: String,
    entries: &BTreeSet<Entry>,
    a: Option<&Attachment>,
) {
    let on = match a {
        Some(a) => match a.part {
            Part::Alias(i) => On::Alias(a.record.clone(), i),
            _ => On::Record(a.record.clone(), a.signature),
        },
        None => On::Unattached,
    };
    let mut needs: Vec<Need> = entries
        .iter()
        .map(|e| match e {
            Entry::Event(n) => Need::Event(n.clone(), on.clone()),
            Entry::Remote(ty, ev) => Need::Remote(ty.clone(), ev.clone()),
            Entry::Custom => Need::Custom,
            Entry::Slot(s) => {
                let Some(a) = a else { return Need::Unreached };
                let (r, sg) = (a.record.clone(), a.signature);
                match (s, &sg) {
                    (Slot::Stage(n, _), _) => Need::Stage(r, i32::from(*n)),
                    (Slot::Flag(bit), b"INFO") => Need::Line(r, *bit == FRAGMENT_END),
                    (Slot::Flag(bit), b"SCEN") => Need::Scene(
                        r,
                        if *bit == FRAGMENT_END {
                            ScenePoint::End
                        } else {
                            ScenePoint::Begin
                        },
                    ),
                    (Slot::Flag(bit), _) => Need::Package(r, *bit),
                    (Slot::Phase(p, f), _) => Need::Scene(r, ScenePoint::Phase(*p, *f)),
                    (Slot::Perk(e), _) => Need::Perk(r, *e),
                    (Slot::Terminal(i), _) => Need::Terminal(r, *i),
                }
            }
        })
        .collect();
    if needs.is_empty() {
        needs.push(Need::Unreached);
    }
    let kind = if site.depth == 0 {
        format!("a call in {attach}")
    } else {
        format!("a call in {attach}, through a function passing a parameter on")
    };
    let site_text = format!(
        "{}.{} ({})",
        site.func.script,
        site.func.name,
        a.map(|a| bind::record_key(&a.record))
            .unwrap_or_else(|| "unattached".into())
    );
    for need in needs {
        if site.start {
            let mut quests: Vec<&Resolved<FormKey>> = pairs.iter().map(|p| &p.0).collect();
            quests.dedup();
            for quest in quests {
                t.starters.push(Starter {
                    quest: quest.clone(),
                    kind: format!("a Start call in {attach}"),
                    need: Some(need.clone()),
                    site: site_text.clone(),
                });
            }
            continue;
        }
        for (quest, s) in &pairs {
            t.setters.push(Setter {
                quest: quest.clone(),
                stage: s.clone(),
                kind: kind.clone(),
                need: need.clone(),
                depth: site.depth,
                site: site_text.clone(),
            });
        }
    }
}

// ---- Record fields -------------------------------------------------------

/// The INFO and SCEN stage fields (`record_set_stage_fields_v1`): an INFO's
/// TIQS and a scene's and a phase's SCQS, two i16 each (the first at the
/// begin, the second at the end; -1 none), on the owning quest; and the
/// startup stages of every quest (`quest_start_v1`).
fn fields(t: &mut Table, store: &Store, b: &Budget) -> Result<()> {
    let mut owners: BTreeMap<String, FormKey> = BTreeMap::new();
    for (o, cell, kind) in store.placement_groups() {
        if kind == 10 && !o.deleted && o.signature == *b"INFO" {
            owners.insert(
                bind::record_key(&store.overrides().form_key(o.key, store.order())),
                cell,
            );
        }
    }
    let keys: Vec<(FormKey, [u8; 4])> = store
        .winners()
        .filter(|w| !w.deleted && matches!(&w.signature, b"INFO" | b"SCEN" | b"QUST"))
        .map(|w| {
            (
                store.overrides().form_key(w.key, store.order()),
                w.signature,
            )
        })
        .collect();
    for (k, s) in keys {
        let bb = b.child();
        match &s {
            b"INFO" => {
                let Some(r) = store.raw(&k, &bb)? else {
                    continue;
                };
                for x in r.subrecords.iter().filter(|x| x.signature == *b"TIQS") {
                    let Some(pair) = pair(r.data(x)) else {
                        t.count("record fields not read as a pair\tINFO TIQS");
                        continue;
                    };
                    let quest = match owners.get(&bind::record_key(&k)) {
                        Some(q) => Resolved::Const(q.clone()),
                        None => Resolved::Open("the line's owning quest is not read"),
                    };
                    for (end, v) in [(false, pair[0]), (true, pair[1])] {
                        let half = if end { "second" } else { "first" };
                        if v < 0 {
                            t.count(format!(
                                "record stage fields\tINFO TIQS\t{half} half\tnegative (none)"
                            ));
                            continue;
                        }
                        t.setters.push(Setter {
                            quest: quest.clone(),
                            stage: Resolved::Const(i32::from(v)),
                            kind: format!(
                                "a dialogue line (INFO) field TIQS, {} half",
                                if end { "second" } else { "first" }
                            ),
                            need: Need::Line(k.clone(), end),
                            depth: 0,
                            site: format!("INFO {}", bind::record_key(&k)),
                        });
                    }
                }
            }
            b"SCEN" => {
                let Some((r, plugin)) = store.raw_with_plugin(&k, &bb)? else {
                    continue;
                };
                let ctx = fo4_plugin::actor::Context {
                    order: store.order(),
                    plugin,
                };
                let Ok(sc) = fo4_plugin::scen::Scene::read(&r, ctx, &bb) else {
                    t.count("scenes that do not read");
                    continue;
                };
                let quest = match &sc.quest.key {
                    Some(q) => Resolved::Const(q.clone()),
                    None => Resolved::Open("the scene's owning quest does not resolve"),
                };
                let put = |t: &mut Table, data: &[u8], what: &str, points: [ScenePoint; 2]| {
                    let Some(pair) = pair(data) else {
                        *t.counts
                            .entry(format!("record fields not read as a pair\t{what}"))
                            .or_default() += 1;
                        return;
                    };
                    for (i, v) in pair.into_iter().enumerate() {
                        let half = if i == 1 { "second" } else { "first" };
                        if v < 0 {
                            *t.counts
                                .entry(format!(
                                    "record stage fields\t{what}\t{half} half\tnegative (none)"
                                ))
                                .or_default() += 1;
                            continue;
                        }
                        t.setters.push(Setter {
                            quest: quest.clone(),
                            stage: Resolved::Const(i32::from(v)),
                            kind: format!(
                                "{what}, {} half",
                                if i == 1 { "second" } else { "first" }
                            ),
                            need: Need::Scene(k.clone(), points[i]),
                            depth: 0,
                            site: format!("SCEN {}", bind::record_key(&k)),
                        });
                    }
                };
                for f in sc.fields.iter().filter(|f| f.tag == *b"SCQS") {
                    let Some(x) = r.subrecords.get(f.index) else {
                        continue;
                    };
                    match f.place {
                        "scene" => put(
                            t,
                            r.data(x),
                            "a scene field SCQS",
                            [ScenePoint::Begin, ScenePoint::End],
                        ),
                        "phase" => {}
                        p => {
                            *t.counts
                                .entry(format!(
                                "record fields SCQS not read as a stage field\tplace {p}, {} bytes",
                                f.size
                            ))
                                .or_default() += 1
                        }
                    }
                }
                for (i, p) in sc.phases.iter().enumerate() {
                    if let Some(d) = &p.scqs {
                        let i = i as u32;
                        put(
                            t,
                            d,
                            "a scene phase field SCQS",
                            [
                                ScenePoint::PhaseField(i, false),
                                ScenePoint::PhaseField(i, true),
                            ],
                        );
                    }
                }
            }
            _ => {
                let Some(q) = QuestInfo::load(store, &k, &bb)? else {
                    continue;
                };
                t.stages.insert(
                    bind::record_key(&k),
                    (
                        k.clone(),
                        q.stages.iter().map(|s| (s.stage, s.flags)).collect(),
                    ),
                );
                for s in q.stages.iter().filter(|s| s.flags & STARTUP_STAGE != 0) {
                    t.setters.push(Setter {
                        quest: Resolved::Const(k.clone()),
                        stage: Resolved::Const(i32::from(s.stage)),
                        kind: "a startup stage (INDX flag 0x2)".into(),
                        need: Need::QuestStart(k.clone()),
                        depth: 0,
                        site: format!("QUST {}", bind::record_key(&k)),
                    });
                }
                if q.start_game_enabled() {
                    t.starters.push(Starter {
                        quest: Resolved::Const(k.clone()),
                        kind: "flagged to start with the game (DNAM bit 0)".into(),
                        need: None,
                        site: format!("QUST {}", bind::record_key(&k)),
                    });
                }
            }
        }
    }
    Ok(())
}

/// Two little-endian i16 of a four-byte field.
fn pair(d: &[u8]) -> Option<[i16; 2]> {
    let d: [u8; 4] = d.try_into().ok()?;
    Some([
        i16::from_le_bytes([d[0], d[1]]),
        i16::from_le_bytes([d[2], d[3]]),
    ])
}

/// The quests a story manager node lists, as starters.
fn starts(t: &mut Table, store: &Store, tree: Option<&Tree>, _b: &Budget) -> Result<()> {
    let Some(tree) = tree else { return Ok(()) };
    let _ = store;
    for (k, n) in &tree.nodes {
        let Some(ev) = tree.event_of(k) else { continue };
        for q in n.quest_keys() {
            t.starters.push(Starter {
                quest: Resolved::Const(q.clone()),
                kind: format!(
                    "a story manager node of event {}",
                    String::from_utf8_lossy(&ev)
                ),
                need: None,
                site: format!("node {}", bind::record_key(k)),
            });
        }
    }
    Ok(())
}

/// Every winning compiled script of the mounted archives, in a library.
pub fn read_all_scripts(vfs: &fo4_vfs::Vfs, max_memory: u64) -> Result<MapLibrary> {
    let mut lib = MapLibrary::default();
    let cands = &vfs.candidates;
    for (i, x) in cands.iter().enumerate() {
        let Some(k) = &x.key else { continue };
        if !k.ends_with(b".pex") || cands.get(i + 1).and_then(|n| n.key.as_ref()) == Some(k) {
            continue;
        }
        let bb = Budget::new(max_memory);
        let bytes = vfs.read_candidate(x, &bb)?;
        let path = String::from_utf8_lossy(k).into_owned();
        let p = fo4_pex::Pex::read(&bytes, &path, &bb)?;
        if let Some(s) = Script::from_pex(&p, 0) {
            lib.add(s);
        }
    }
    Ok(lib)
}

/// The quest whose group of kind 10 holds each INFO and SCEN.
pub fn owners(store: &Store) -> BTreeMap<String, FormKey> {
    let mut out = BTreeMap::new();
    for (o, cell, kind) in store.placement_groups() {
        if kind == 10 && !o.deleted && matches!(&o.signature, b"INFO" | b"SCEN") {
            out.insert(
                bind::record_key(&store.overrides().form_key(o.key, store.order())),
                cell,
            );
        }
    }
    out
}

// ---- What a stopped run waits on ------------------------------------------

/// A stage a scene stopped in a phase waits on: a condition of function 59
/// (`cond_fn_59_v1`: the stage of the quest the first parameter names, the
/// second parameter, done) or 58 compared with `==` or `>=`
/// (`cond_fn_58_v1`: the quest's current stage, the comparison value as the
/// stage), in the phase's start or completion list, whose stage the run
/// has not set.
#[derive(Debug, Clone, PartialEq)]
pub struct Wait {
    pub scene: crate::ObjId,
    pub phase: usize,
    /// `start` or `completion`.
    pub list: &'static str,
    pub function: u16,
    pub quest: FormKey,
    pub stage: i32,
    /// Whether the run holds the stage done; `None` when undetermined.
    pub done: Option<bool>,
}

/// The scenes of a run stopped in a phase at its end, with the phase and
/// the reason: every scene whose last change is a stop.
pub fn scene_stops(w: &crate::World) -> BTreeMap<crate::ObjId, (usize, String)> {
    use crate::scene::Change;
    let mut last: BTreeMap<crate::ObjId, (usize, String)> = BTreeMap::new();
    for (o, ch) in &w.scenes.log {
        match ch {
            Change::Waiting(p, why) => {
                last.insert(*o, (*p, why.clone()));
            }
            _ => {
                last.remove(o);
            }
        }
    }
    last
}

/// The stages the scenes of a run wait on at its end: every scene whose
/// last change is a stop in a phase on its conditions.
pub fn scene_waits(w: &mut crate::World, store: &Store) -> Vec<Wait> {
    let last = scene_stops(w);
    let max = store.limits.max_memory;
    let mut out = Vec::new();
    for (o, (p, why)) in last {
        let list = if why.starts_with("completion conditions") {
            "completion"
        } else if why.starts_with("start conditions") {
            "start"
        } else {
            continue;
        };
        let Some(info) = crate::scene::info(store, max, w, o) else {
            continue;
        };
        let Some(phase) = info.phases.get(p) else {
            continue;
        };
        let conds = if list == "start" {
            &phase.start
        } else {
            &phase.completion
        };
        let ctx = fo4_plugin::actor::Context {
            order: store.order(),
            plugin: info.plugin,
        };
        for c in conds {
            let f = c.ctda.function;
            let stage = match f {
                59 => i32::try_from(c.ctda.param2).ok(),
                58 if matches!(c.ctda.operator(), 0 | 3) => match c.ctda.comparison() {
                    fo4_plugin::condition::Comparison::Float(x) if x.fract() == 0.0 => {
                        Some(x as i32)
                    }
                    _ => None,
                },
                _ => None,
            };
            let Some(stage) = stage else { continue };
            let Ok(Some(q)) = fo4_plugin::condition::form(ctx, c.ctda.param1, &info.file, 0) else {
                continue;
            };
            let Some(qi) = crate::quest::info_by_key(store, max, w, &q) else {
                continue;
            };
            let done = w.quests.state(&qi).is_done(stage);
            if done == Some(true) {
                continue;
            }
            out.push(Wait {
                scene: o,
                phase: p,
                list,
                function: f,
                quest: q,
                stage,
                done,
            });
        }
    }
    out
}
