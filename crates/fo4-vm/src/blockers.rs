//! The condition lists a run evaluates, by context, and what leaves each
//! one undetermined (`spec/conditions.md`, Blockers; the `cond_blockers`
//! census): scene phase start and completion conditions, quest stage log
//! entry conditions, quest alias fill conditions, a quest's story-event
//! conditions (after NEXT) and story manager node conditions.
//!
//! A list left undetermined is **blocked only by** a function index when
//! every condition that leaves it open is of that index and the index is
//! not evaluated ([`cond::NOT_EVALUATED`]): evaluating that one index,
//! whatever it answers, decides the list.
use crate::{
    cond::{Truth, NOT_EVALUATED},
    scene::SceneInfo,
};
use fo4_core::{Budget, Result};
use fo4_plugin::{
    actor::Context,
    condition::Condition,
    quest::{AliasValue, Quest},
    story::Tree,
    typed::Store,
    FormKey,
};
use std::collections::BTreeSet;

/// The contexts, in census order.
pub const CONTEXTS: [&str; 6] = [
    "scene phase start",
    "scene phase completion",
    "quest stage log entry",
    "quest alias fill",
    "quest story event (after NEXT)",
    "story manager node",
];

/// One list with where it is.
#[derive(Debug, Clone)]
pub struct ListAt {
    /// One of [`CONTEXTS`].
    pub context: &'static str,
    /// The record type that holds it.
    pub host: [u8; 4],
    pub key: FormKey,
    /// The quest whose aliases run-on 5 names (`cond_run_on_subject_v1`):
    /// the quest itself, a scene's owning quest (PNAM); none for a node.
    pub owner: Option<FormKey>,
    pub plugin: usize,
    pub file: String,
    /// Where in the record: `phase N`, `stage N entry K`, `alias ID`,
    /// `after NEXT`, `node`.
    pub place: String,
    pub list: Vec<Condition>,
}

/// The phase start and completion lists of a scene that has them.
pub fn of_scene(store: &Store, k: &FormKey, b: &Budget) -> Result<Vec<ListAt>> {
    let Some(s) = SceneInfo::load(store, k, b)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (p, ph) in s.phases.iter().enumerate() {
        for (context, l) in [(CONTEXTS[0], &ph.start), (CONTEXTS[1], &ph.completion)] {
            if !l.is_empty() {
                out.push(ListAt {
                    context,
                    host: *b"SCEN",
                    key: k.clone(),
                    owner: s.quest.clone(),
                    plugin: s.plugin,
                    file: s.file.clone(),
                    place: format!("phase {p}"),
                    list: l.clone(),
                });
            }
        }
    }
    Ok(out)
}

/// The log entry, alias fill and after-NEXT lists of a quest. The
/// conditions of one alias are one list.
pub fn of_quest(store: &Store, k: &FormKey, b: &Budget) -> Result<Vec<ListAt>> {
    if store.signature(k) != Some(*b"QUST") {
        return Ok(Vec::new());
    }
    let Some((r, plugin)) = store.raw_with_plugin(k, b)? else {
        return Ok(Vec::new());
    };
    let ctx = Context {
        order: store.order(),
        plugin,
    };
    let q = Quest::read(&r, ctx, b)?;
    let mut out = Vec::new();
    let mut push = |context: &'static str, place: String, list: Vec<Condition>| {
        if !list.is_empty() {
            out.push(ListAt {
                context,
                host: *b"QUST",
                key: k.clone(),
                owner: Some(k.clone()),
                plugin,
                file: r.source.file.clone(),
                place,
                list,
            });
        }
    };
    let conv = |l: &[fo4_plugin::quest::Condition]| -> Vec<Condition> {
        l.iter().map(Condition::from).collect()
    };
    for s in &q.stages {
        for (n, e) in s.entries.iter().enumerate() {
            push(
                CONTEXTS[2],
                format!("stage {} entry {n}", s.stage),
                conv(&e.conditions),
            );
        }
    }
    for al in &q.aliases {
        let v: Vec<Condition> = al
            .values
            .iter()
            .filter_map(|v| match v {
                AliasValue::Condition(c) => Some(Condition::from(c)),
                _ => None,
            })
            .collect();
        push(CONTEXTS[3], format!("alias {}", al.id), v);
    }
    push(CONTEXTS[4], "after NEXT".into(), conv(&q.event_conditions));
    Ok(out)
}

/// The condition lists of the story manager's nodes.
pub fn of_tree(tree: &Tree) -> Vec<ListAt> {
    tree.nodes
        .iter()
        .filter(|(_, n)| !n.conditions.is_empty())
        .map(|(k, n)| ListAt {
            context: CONTEXTS[5],
            host: n.kind.signature(),
            key: k.clone(),
            owner: None,
            plugin: n.plugin,
            file: n.file.clone(),
            place: "node".into(),
            list: n.conditions.clone(),
        })
        .collect()
}

/// What leaves an undetermined list open.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Blockers {
    /// Function indexes the evaluator does not evaluate (or whose switch
    /// is off).
    pub not_evaluated: BTreeSet<u16>,
    /// Evaluated function indexes whose inputs the state leaves open, with
    /// the reason each gives.
    pub open: BTreeSet<(u16, &'static str)>,
}
impl Blockers {
    /// The blockers of an undetermined list; `None` for a decided one.
    pub fn of(t: &Truth) -> Option<Self> {
        let Truth::Undetermined(why) = t else {
            return None;
        };
        let mut b = Blockers::default();
        for w in why {
            if w.why == NOT_EVALUATED {
                b.not_evaluated.insert(w.function);
            } else {
                b.open.insert((w.function, w.why));
            }
        }
        Some(b)
    }
    /// The one index that alone blocks the list, if there is one.
    pub fn sole(&self) -> Option<u16> {
        if self.not_evaluated.len() == 1 && self.open.is_empty() {
            self.not_evaluated.iter().next().copied()
        } else {
            None
        }
    }
    /// The evaluated function indexes left open.
    pub fn open_functions(&self) -> BTreeSet<u16> {
        self.open.iter().map(|x| x.0).collect()
    }
    /// A short class for the census.
    pub fn shape(&self) -> &'static str {
        match (self.not_evaluated.len(), self.open.is_empty()) {
            (0, _) => "only evaluated functions whose input the state leaves open",
            (1, true) => "one function index not evaluated, alone",
            (1, false) => "one function index not evaluated, with open evaluated functions",
            (_, true) => "two or more function indexes not evaluated",
            (_, false) => {
                "two or more function indexes not evaluated, with open evaluated functions"
            }
        }
    }
}
