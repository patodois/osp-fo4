//! Conditions (`spec/conditions.md`, `spec/vm.md`, Conditions): a
//! three-valued evaluator over the CTDA lists read by
//! `fo4_plugin::condition`. A condition is true, false or undetermined;
//! only the functions in [`EVALUATED`] are evaluated, from the records and
//! the quest state a run keeps ([`Facts`]); every other function, and an
//! evaluated one whose inputs nothing settles, is undetermined with its
//! reason. A list is undetermined only when its determined conditions do
//! not already settle it (Kleene's three-valued AND and OR).
//!
//! Every meaning here is a pending rule (`cond_*` in `spec/hypotheses.tsv`):
//! how a list combines, what the operator code compares, the global bit,
//! the flag bits that make parameters name aliases or package data, and
//! each function index's meaning. None is measured in the running game,
//! and no public table of function indexes was read: the meanings of the
//! evaluated indexes were recognised from prior knowledge and kept because
//! the census of their parameters (`cond_census`) could have refuted them
//! and did not.
use crate::{data, rules};
use fo4_plugin::{
    actor::Context,
    condition::{self, flag, Comparison, Condition, Ctda},
    typed::Store,
    FormKey,
};
use std::collections::{BTreeMap, BTreeSet};

/// Why one condition is undetermined.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Why {
    /// The function index.
    pub function: u16,
    pub why: &'static str,
}

/// The value of a condition or a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Truth {
    True,
    False,
    /// The undetermined conditions that leave it open.
    Undetermined(Vec<Why>),
}
impl Truth {
    pub fn word(&self) -> &'static str {
        match self {
            Truth::True => "true",
            Truth::False => "false",
            Truth::Undetermined(_) => "undetermined",
        }
    }
    fn of(b: bool) -> Self {
        if b {
            Truth::True
        } else {
            Truth::False
        }
    }
    fn open(function: u16, why: &'static str) -> Self {
        Truth::Undetermined(vec![Why { function, why }])
    }
}

/// How the conditions of a list combine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combine {
    /// The candidate (`cond_list_or_groups_v1`): a condition with the OR
    /// bit is joined to the next by OR, others by AND, and OR binds
    /// tighter: the list is the AND of its OR groups.
    OrGroups,
    /// The alternative counted by the census: the connectors applied from
    /// left to right with no precedence.
    LeftToRight,
}

/// Function indexes the evaluator evaluates, with the rule each rests on.
/// The meanings were recognised from prior knowledge (no public table was
/// read); the census of their parameters supports them (`spec/conditions.md`).
pub const EVALUATED: [(u16, &str); 4] = [
    (56, rules::COND_FN_56),
    (58, rules::COND_FN_58),
    (59, rules::COND_FN_59),
    (74, rules::COND_FN_74),
];

/// The rule of an evaluated function index.
pub fn rule_of(function: u16) -> Option<&'static str> {
    EVALUATED
        .iter()
        .find(|(f, _)| *f == function)
        .map(|(_, r)| *r)
}

/// What an evaluation reads: the store, and the state of quests and
/// globals. An `Err` is the reason the state does not settle it.
pub trait Facts {
    fn store(&self) -> &Store;
    /// Whether the quest is running.
    fn quest_running(&mut self, quest: &FormKey) -> Result<bool, &'static str>;
    /// The quest's current stage.
    fn quest_stage(&mut self, quest: &FormKey) -> Result<i32, &'static str>;
    /// Whether the stage of the quest is done.
    fn stage_done(&mut self, quest: &FormKey, stage: i32) -> Result<bool, &'static str>;
    /// The value of a global.
    fn global(&mut self, global: &FormKey) -> Result<f32, &'static str>;
}

/// The winning record of type `sig` a word names, read as a FormID
/// through the record's plugin.
fn record(
    facts: &dyn Facts,
    ctx: Context<'_>,
    raw: u32,
    file: &str,
    sig: &[u8; 4],
) -> Result<FormKey, &'static str> {
    let k = condition::form(ctx, raw, file, 0)
        .map_err(|_| "a parameter is not a FormID")?
        .ok_or("a parameter is zero")?;
    if facts.store().signature(&k) != Some(*sig) {
        return Err(match sig {
            b"QUST" => "a parameter names no winning QUST",
            _ => "a parameter names no winning GLOB",
        });
    }
    Ok(k)
}

/// The function's value of one condition, as a number.
fn function_value(
    c: &Ctda,
    ctx: Context<'_>,
    file: &str,
    facts: &mut dyn Facts,
) -> Result<f32, &'static str> {
    let b = |v: bool| if v { 1.0 } else { 0.0 };
    match c.function {
        56 => {
            let q = record(facts, ctx, c.param1, file, b"QUST")?;
            facts.quest_running(&q).map(b)
        }
        58 => {
            let q = record(facts, ctx, c.param1, file, b"QUST")?;
            facts.quest_stage(&q).map(|n| n as f32)
        }
        59 => {
            let q = record(facts, ctx, c.param1, file, b"QUST")?;
            let n = i32::try_from(c.param2).map_err(|_| "the stage parameter is out of range")?;
            facts.stage_done(&q, n).map(b)
        }
        74 => {
            let g = record(facts, ctx, c.param1, file, b"GLOB")?;
            facts.global(&g)
        }
        _ => Err("the function is not evaluated"),
    }
}

/// Evaluates one condition; adds the rules it relied on to `used`.
pub fn eval_term(
    c: &Ctda,
    ctx: Context<'_>,
    file: &str,
    facts: &mut dyn Facts,
    used: &mut BTreeSet<&'static str>,
) -> Truth {
    used.insert(rules::COND_LAYOUT);
    let f = c.function;
    let Some(rule) = rule_of(f) else {
        return Truth::open(f, "the function is not evaluated");
    };
    used.insert(rules::COND_FLAGS);
    if c.flags() & (flag::ALIASES | flag::PACK_DATA) != 0 {
        return Truth::open(f, "the parameters name aliases or package data");
    }
    used.insert(rule);
    let v = match function_value(c, ctx, file, facts) {
        Ok(v) => v,
        Err(why) => return Truth::open(f, why),
    };
    let against = match c.comparison() {
        Comparison::Float(x) => x,
        Comparison::Global(raw) => {
            used.insert(rules::COND_GLOBAL);
            match record(facts, ctx, raw, file, b"GLOB").and_then(|g| facts.global(&g)) {
                Ok(x) => x,
                Err(why) => return Truth::open(f, why),
            }
        }
    };
    if !v.is_finite() || !against.is_finite() {
        return Truth::open(f, "a value is not finite");
    }
    used.insert(rules::COND_OPS);
    match c.operator() {
        0 => Truth::of(v == against),
        1 => Truth::of(v != against),
        2 => Truth::of(v > against),
        3 => Truth::of(v >= against),
        4 => Truth::of(v < against),
        5 => Truth::of(v <= against),
        _ => Truth::open(f, "an operator code without a candidate meaning"),
    }
}

/// [`eval_term`] with the rules it relied on.
pub fn eval_one(
    c: &Ctda,
    ctx: Context<'_>,
    file: &str,
    facts: &mut dyn Facts,
) -> (Truth, BTreeSet<&'static str>) {
    let mut used = BTreeSet::new();
    let t = eval_term(c, ctx, file, facts, &mut used);
    (t, used)
}

fn and3(a: Truth, b: Truth) -> Truth {
    match (a, b) {
        (Truth::False, _) | (_, Truth::False) => Truth::False,
        (Truth::True, x) | (x, Truth::True) => x,
        (Truth::Undetermined(mut x), Truth::Undetermined(y)) => {
            x.extend(y);
            Truth::Undetermined(x)
        }
    }
}
fn or3(a: Truth, b: Truth) -> Truth {
    match (a, b) {
        (Truth::True, _) | (_, Truth::True) => Truth::True,
        (Truth::False, x) | (x, Truth::False) => x,
        (Truth::Undetermined(mut x), Truth::Undetermined(y)) => {
            x.extend(y);
            Truth::Undetermined(x)
        }
    }
}
fn tidy(t: Truth) -> Truth {
    match t {
        Truth::Undetermined(mut w) => {
            w.sort();
            w.dedup();
            Truth::Undetermined(w)
        }
        t => t,
    }
}

/// Evaluates a list (`cond_list_or_groups_v1` under [`Combine::OrGroups`]);
/// an empty list holds. Adds the rules relied on to `used`.
pub fn eval_list_with(
    list: &[Condition],
    ctx: Context<'_>,
    file: &str,
    facts: &mut dyn Facts,
    combine: Combine,
    used: &mut BTreeSet<&'static str>,
) -> Truth {
    if list.is_empty() {
        return Truth::True;
    }
    used.insert(rules::COND_LIST);
    let terms: Vec<Truth> = list
        .iter()
        .map(|c| eval_term(&c.ctda, ctx, file, facts, used))
        .collect();
    let t = match combine {
        Combine::OrGroups => {
            let mut all = Truth::True;
            let mut group = Truth::False;
            let n = terms.len();
            for (i, (t, c)) in terms.into_iter().zip(list).enumerate() {
                group = or3(group, t);
                if !c.ctda.or() || i + 1 == n {
                    all = and3(all, std::mem::replace(&mut group, Truth::False));
                }
            }
            all
        }
        Combine::LeftToRight => {
            let mut it = terms.into_iter().zip(list);
            let Some((mut acc, mut prev)) = it.next() else {
                return Truth::True;
            };
            for (t, c) in it {
                acc = if prev.ctda.or() {
                    or3(acc, t)
                } else {
                    and3(acc, t)
                };
                prev = c;
            }
            acc
        }
    };
    tidy(t)
}

/// [`eval_list_with`] without the rules.
pub fn eval_list(
    list: &[Condition],
    ctx: Context<'_>,
    file: &str,
    facts: &mut dyn Facts,
    combine: Combine,
) -> Truth {
    eval_list_with(list, ctx, file, facts, combine, &mut BTreeSet::new())
}

/// Facts of the new-game scenario before any script ran
/// (`quest_start_game_enabled_v1`): a quest runs exactly when its DNAM
/// carries the start-game bit, no stage is done, the current stage is
/// undetermined, a global holds its stored value
/// (`vm_host_global_fltv_v1`, `vm_host_data_state_v1`).
pub struct StaticFacts<'s> {
    pub store: &'s Store,
    pub max_memory: u64,
    running: BTreeMap<FormKey, Result<bool, &'static str>>,
}
impl<'s> StaticFacts<'s> {
    pub fn new_game(store: &'s Store, max_memory: u64) -> Self {
        StaticFacts {
            store,
            max_memory,
            running: BTreeMap::new(),
        }
    }
}
impl Facts for StaticFacts<'_> {
    fn store(&self) -> &Store {
        self.store
    }
    fn quest_running(&mut self, quest: &FormKey) -> Result<bool, &'static str> {
        if let Some(r) = self.running.get(quest) {
            return *r;
        }
        let b = fo4_core::Budget::new(self.max_memory);
        let r = match crate::quest::QuestInfo::load(self.store, quest, &b) {
            Ok(Some(i)) => Ok(i.start_game_enabled()),
            _ => Err("the quest record could not be read"),
        };
        self.running.insert(quest.clone(), r);
        r
    }
    fn quest_stage(&mut self, _: &FormKey) -> Result<i32, &'static str> {
        Err("the current stage is undetermined until a stage is set")
    }
    fn stage_done(&mut self, _: &FormKey, _: i32) -> Result<bool, &'static str> {
        Ok(false)
    }
    fn global(&mut self, global: &FormKey) -> Result<f32, &'static str> {
        data::global_fltv(self.store, global, self.max_memory)
    }
}
