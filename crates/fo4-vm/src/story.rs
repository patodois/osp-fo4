//! The story manager at run time (`spec/story-manager.md`): the script
//! event `Keyword.SendStoryEvent` raises, offered to the tree of nodes
//! `fo4_plugin::story` reads, and the quest a quest node starts. Only this
//! event is raised: every other event code names something the world does
//! (a location change, a dialogue, a kill), which no run here performs.
//!
//! Every behaviour is a pending rule: `sm_script_event_v1` (what the call
//! raises and answers), `sm_tree_walk_v1` (how the walk goes through the
//! tree), `sm_quest_node_start_v1` (what a quest node starts),
//! `cond_fn_576_event_data_v1` (the event members a node's conditions
//! read). The stored flag words of the nodes (DNAM, XNAM, MNAM, HNAM,
//! RNAM) are not read: the walk is the same whatever they hold, which is
//! part of what the rules state and a measurement could refute.
use crate::{
    cond::{self, Combine, Truth},
    data::{key_record, record_object},
    host::{NativeCall, Reply},
    quest::{self, Job, QuestHost, RunFacts},
    rules,
    world::{Val, World},
};
use fo4_core::Budget;
use fo4_plugin::{
    actor::Context,
    condition::Condition,
    story::{Kind, Order, Tree},
    FormKey,
};
use std::collections::BTreeSet;

/// The event code of the script event (`sm_script_event_v1`).
pub const SCRIPT_EVENT: [u8; 4] = *b"SCPT";

/// The natives that raise it.
pub const NATIVES: [&str; 2] = ["keyword.sendstoryevent", "keyword.sendstoryeventandwait"];

/// One story event: its code and the members the conditions may read.
#[derive(Debug, Clone)]
pub struct StoryEvent {
    pub code: [u8; 4],
    /// The keyword (`K1`).
    pub keyword: Option<FormKey>,
    /// The location (`L1`); `Err` when the call's argument is undetermined.
    pub location: Result<Option<FormKey>, &'static str>,
    /// The values the started quest's `OnStoryScript` receives.
    pub values: Vec<Val>,
}

/// What offering an event to a tree gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Walk {
    /// Nothing here starts a quest.
    Skip,
    /// This quest is started.
    Start(FormKey),
    /// The rules do not settle it: why, and the function indexes of the
    /// undetermined conditions on the way.
    Open(&'static str, BTreeSet<u16>),
}

fn eval(
    host: &mut QuestHost<'_>,
    w: &mut World,
    ev: &StoryEvent,
    list: &[Condition],
    plugin: usize,
    file: &str,
    owner: Option<FormKey>,
) -> Truth {
    let ctx = Context {
        order: host.data.store.order(),
        plugin,
    };
    let mut used = BTreeSet::new();
    let t = {
        let mut facts = RunFacts::of(host, w, Some(ev), owner);
        cond::eval_list_with(list, ctx, file, &mut facts, Combine::OrGroups, &mut used)
    };
    for u in used {
        host.using(u);
    }
    t
}

fn functions(t: &Truth) -> BTreeSet<u16> {
    match t {
        Truth::Undetermined(w) => w.iter().map(|x| x.function).collect(),
        _ => BTreeSet::new(),
    }
}

/// Walks the node `k` (`sm_tree_walk_v1`, `sm_quest_node_start_v1`): its
/// conditions false: nothing; undetermined: open; true: a quest node
/// offers its quests in stored order, the first not running whose
/// conditions after NEXT hold is started; an event or branch node offers
/// the event to its children in previous-link order, the first child that
/// starts a quest ends the walk, a child left open leaves the walk open.
/// Under a parent whose links do not make one chain, the children are all
/// walked: open when one is open or more than one would start a quest.
pub fn walk(
    host: &mut QuestHost<'_>,
    w: &mut World,
    tree: &Tree,
    k: &FormKey,
    ev: &StoryEvent,
    depth: usize,
) -> Walk {
    let Some(n) = tree.nodes.get(k) else {
        return Walk::Open("a node the tree names was not read", BTreeSet::new());
    };
    if depth > fo4_plugin::story::MAX_DEPTH {
        return Walk::Open("the tree is deeper than the walk follows", BTreeSet::new());
    }
    let t = eval(host, w, ev, &n.conditions, n.plugin, &n.file, None);
    match t {
        Truth::False => return Walk::Skip,
        Truth::Undetermined(_) => {
            return Walk::Open("a node's conditions are undetermined", functions(&t))
        }
        Truth::True => {}
    }
    if n.kind == Kind::Quest {
        host.using(rules::SM_QUEST_START);
        for q in n.quest_keys() {
            let Some(i) = quest::info_by_key(host.data.store, host.data.max_memory, w, q) else {
                return Walk::Open("a quest the node lists could not be read", BTreeSet::new());
            };
            let st = w.quests.state(&i);
            if st.open.is_some() {
                return Walk::Open("a listed quest's state is undetermined", BTreeSet::new());
            }
            match st.running {
                Some(true) => continue,
                None => {
                    return Walk::Open(
                        "whether a listed quest runs is undetermined",
                        BTreeSet::new(),
                    )
                }
                Some(false) => {}
            }
            let t = eval(
                host,
                w,
                ev,
                &i.event_conditions,
                i.plugin,
                &i.file,
                Some(i.key.clone()),
            );
            match t {
                Truth::True => return Walk::Start(q.clone()),
                Truth::False => continue,
                Truth::Undetermined(_) => {
                    return Walk::Open(
                        "a listed quest's conditions are undetermined",
                        functions(&t),
                    )
                }
            }
        }
        return Walk::Skip;
    }
    let (kids, order) = tree.children_of(k);
    let kids = kids.to_vec();
    if order == Order::Chain {
        for c in &kids {
            match walk(host, w, tree, c, ev, depth + 1) {
                Walk::Skip => continue,
                r => return r,
            }
        }
        return Walk::Skip;
    }
    let acting: Vec<Walk> = kids
        .iter()
        .map(|c| walk(host, w, tree, c, ev, depth + 1))
        .filter(|r| *r != Walk::Skip)
        .collect();
    unordered(
        acting,
        "siblings whose order the previous links do not settle would both start a quest",
    )
}

/// The result of walks without a settled order: nothing when none acts,
/// the one that acts, the first open one when one is open, else open.
fn unordered(acting: Vec<Walk>, why: &'static str) -> Walk {
    if let Some(Walk::Open(w, _)) = acting.iter().find(|r| matches!(r, Walk::Open(..))) {
        let fs = acting
            .iter()
            .flat_map(|r| match r {
                Walk::Open(_, f) => f.clone(),
                _ => BTreeSet::new(),
            })
            .collect();
        return Walk::Open(w, fs);
    }
    match acting.len() {
        0 => Walk::Skip,
        1 => acting.into_iter().next().unwrap_or(Walk::Skip),
        _ => Walk::Open(why, BTreeSet::new()),
    }
}

/// Offers an event to the trees of its code: the event nodes of that code
/// walked as siblings without a settled order.
pub fn offer(host: &mut QuestHost<'_>, w: &mut World, tree: &Tree, ev: &StoryEvent) -> Walk {
    host.using(rules::SM_LAYOUT);
    host.using(rules::SM_TREE_WALK);
    let roots: Vec<FormKey> = tree
        .nodes
        .iter()
        .filter(|(_, n)| n.kind == Kind::Event && n.event == Some(ev.code))
        .map(|(k, _)| k.clone())
        .collect();
    let acting: Vec<Walk> = roots
        .iter()
        .map(|r| walk(host, w, tree, r, ev, 0))
        .filter(|r| *r != Walk::Skip)
        .collect();
    unordered(
        acting,
        "two event nodes of the code would both start a quest",
    )
}

fn bump(host: &mut QuestHost<'_>, k: impl Into<String>) {
    *host.story_counts.entry(k.into()).or_default() += 1;
}

/// `Keyword.SendStoryEvent(akLoc, akRef1, akRef2, aiValue1, aiValue2)` and
/// `SendStoryEventAndWait` (`sm_script_event_v1`): the script event with
/// the receiver as its keyword is offered at once; a quest started is
/// started as `Start` starts it and then receives `OnStoryScript(keyword,
/// location, ref1, ref2, value1, value2)`. `SendStoryEventAndWait` answers
/// whether a quest started (undetermined when the walk is open).
pub fn send(host: &mut QuestHost<'_>, c: &NativeCall, w: &mut World) -> Reply {
    host.using(rules::SM_SCRIPT_EVENT);
    let Some(tree) = host.story.clone() else {
        return Reply::Undetermined;
    };
    let wait = c.name == "sendstoryeventandwait";
    let none = |r: Reply| if wait { r } else { Reply::Value(Val::None) };
    bump(host, "story events offered\tSCPT");
    let keyword = match &c.receiver {
        Some(Val::Object(o)) => w.objects[*o].key.as_deref().and_then(key_record),
        _ => None,
    };
    let Some(keyword) = keyword else {
        bump(
            host,
            "story event outcome\topen: the keyword is not a record",
        );
        return none(Reply::Undetermined);
    };
    let location = match c.args.first() {
        Some(Val::None) => Ok(None),
        Some(Val::Object(o)) => w.objects[*o]
            .key
            .as_deref()
            .and_then(key_record)
            .map(Some)
            .ok_or("the location is not a record"),
        _ => Err("the location is undetermined"),
    };
    let mut values = vec![c.receiver.clone().unwrap_or(Val::None)];
    values.extend(c.args.iter().take(5).cloned());
    let ev = StoryEvent {
        code: SCRIPT_EVENT,
        keyword: Some(keyword),
        location,
        values,
    };
    match offer(host, w, &tree, &ev) {
        Walk::Skip => {
            bump(host, "story event outcome\tno quest started");
            none(Reply::Value(Val::Bool(false)))
        }
        Walk::Open(why, fs) => {
            bump(host, format!("story event outcome\topen: {why}"));
            for f in fs {
                bump(
                    host,
                    format!("story event left open, by function index of an undetermined condition\tfunction {f}"),
                );
            }
            none(Reply::Undetermined)
        }
        Walk::Start(q) => {
            let b = Budget::new(host.data.max_memory);
            let Ok(o) = record_object(w, host.data.store, &q, "quest", &b) else {
                bump(
                    host,
                    "story event outcome\topen: the quest could not be read",
                );
                return none(Reply::Undetermined);
            };
            let Some(i) = quest::info_by_key(host.data.store, host.data.max_memory, w, &q) else {
                bump(
                    host,
                    "story event outcome\topen: the quest could not be read",
                );
                return none(Reply::Undetermined);
            };
            bump(host, "story event outcome\ta quest started");
            host.start_quest(w, o, &i, None);
            let depth = w.quests.depth + 1;
            w.quests.queue.push_back(Job::Story {
                quest: o,
                args: ev.values.clone(),
                depth,
            });
            w.quests.log.push((o, quest::Change::StartedByStory));
            none(Reply::Value(Val::Bool(true)))
        }
    }
}
