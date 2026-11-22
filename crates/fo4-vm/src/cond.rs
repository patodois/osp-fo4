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
use crate::{data, program::Library, rules};
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
pub const EVALUATED: [(u16, &str); 5] = [
    (56, rules::COND_FN_56),
    (58, rules::COND_FN_58),
    (59, rules::COND_FN_59),
    (74, rules::COND_FN_74),
    (576, rules::COND_FN_576),
];

/// Function indexes evaluated since the second conditions step, each
/// behind its own switch ([`Switches`]) and its own pending rule.
pub const ADDED: [(u16, &str); 5] = [
    (72, rules::COND_FN_72),
    (84, rules::COND_FN_84),
    (248, rules::COND_FN_248),
    (550, rules::COND_FN_550),
    (629, rules::COND_FN_629),
];

/// The function index read from the scene clock under
/// `cond_fn_131_action_percent_v1`, behind its own switch and off by
/// default (not one of [`ADDED`], which are on by default).
pub const ACTION_PERCENT: u16 = 131;

/// The function index read from the world model's positions under
/// `cond_fn_1_track_distance_v1`, behind its own switch and off by default
/// (not one of [`ADDED`]).
pub const DISTANCE: u16 = 1;

/// The object id the player reference word reads as under
/// `cond_player_reference_word_v1`: a word of the first plugin of the load
/// order naming that id, which no plugin defines.
pub const PLAYER_REFERENCE_ID: u32 = 0x14;

/// One side of a distance: the player or a placed reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Party {
    Player,
    Record(FormKey),
}

/// Record types of placed references, for the reference word of run-on 2.
pub const PLACED: [&[u8; 4]; 10] = [
    b"REFR", b"ACHR", b"PGRE", b"PMIS", b"PHZD", b"PARW", b"PBAR", b"PBEA", b"PCON", b"PFLA",
];

/// The reason a condition of a function index the evaluator does not
/// evaluate gives (or of one whose switch is off).
pub const NOT_EVALUATED: &str = "the function is not evaluated";

/// Which of the evaluators added after the first five are on. Each new
/// function index has its own switch, on by default; with every switch off
/// the evaluator is the one before they were added, and every census it
/// wrote reproduces byte for byte.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Switches {
    off: BTreeSet<u16>,
    /// The run-on reading (`cond_run_on_subject_v1`): when off, every new
    /// evaluator that reads the subject leaves it undetermined.
    pub run_on_off: bool,
}
impl Switches {
    /// Every evaluator on.
    pub const fn all() -> Self {
        Switches {
            off: BTreeSet::new(),
            run_on_off: false,
        }
    }
    /// Every evaluator added after the first five off.
    pub fn none() -> Self {
        let mut s = Self::all();
        for (f, _) in ADDED {
            s.off(f);
        }
        s.run_on_off = true;
        s
    }
    pub fn off(&mut self, function: u16) {
        self.off.insert(function);
    }
    /// Whether the evaluator of an added function index is on.
    pub fn on(&self, function: u16) -> bool {
        !self.off.contains(&function)
    }
    pub fn is_all(&self) -> bool {
        self.off.is_empty() && !self.run_on_off
    }
    /// The switches that are off, as text.
    pub fn describe(&self) -> String {
        let mut v: Vec<String> = self
            .off
            .iter()
            .map(|f| format!("function {f} off"))
            .collect();
        if self.run_on_off {
            v.push("run-on reading off".into());
        }
        v.join(", ")
    }
}

/// Every evaluator on: what [`Facts::switches`] answers by default.
pub static ALL_ON: Switches = Switches::all();

/// The rule of an evaluated function index.
pub fn rule_of(function: u16) -> Option<&'static str> {
    EVALUATED
        .iter()
        .chain(ADDED.iter())
        .find(|(f, _)| *f == function)
        .map(|(_, r)| *r)
}

/// Whether the index is one of the [`ADDED`] evaluators.
pub fn is_added(function: u16) -> bool {
    ADDED.iter().any(|(f, _)| *f == function)
}

/// What an evaluation reads: the store, and the state of quests and
/// globals. An `Err` is the reason the state does not settle it.
pub trait Facts {
    fn store(&self) -> &Store;
    /// Which added evaluators are on.
    fn switches(&self) -> &Switches {
        &ALL_ON
    }
    /// Whether the quest is running.
    fn quest_running(&mut self, quest: &FormKey) -> Result<bool, &'static str>;
    /// The quest's current stage.
    fn quest_stage(&mut self, quest: &FormKey) -> Result<i32, &'static str>;
    /// Whether the stage of the quest is done.
    fn stage_done(&mut self, quest: &FormKey, stage: i32) -> Result<bool, &'static str>;
    /// The value of a global.
    fn global(&mut self, global: &FormKey) -> Result<f32, &'static str>;
    /// The record a member of the story event being offered names (`K1`
    /// its keyword, `L1` its location; `None` for none), for function 576
    /// (`cond_fn_576_event_data_v1`). Outside a story event there is none.
    fn event_member(&mut self, code: [u8; 2]) -> Result<Option<FormKey>, &'static str> {
        let _ = code;
        Err("no story event is being offered")
    }
    /// The quest whose aliases run-on 5 names: the quest of the record the
    /// list belongs to (`cond_run_on_subject_v1`).
    fn owner(&self) -> Option<&FormKey> {
        None
    }
    /// The reference an alias of a quest holds (`None`: empty).
    fn alias_reference(
        &mut self,
        quest: &FormKey,
        alias: i32,
    ) -> Result<Option<FormKey>, &'static str> {
        let _ = (quest, alias);
        Err("alias state is not read here")
    }
    /// The value of a variable (lower case) of the scripts attached to a
    /// quest, as a number (function 629).
    fn quest_variable(&mut self, quest: &FormKey, name: &str) -> Result<f32, &'static str> {
        let _ = (quest, name);
        Err("script variables are not read here")
    }
    /// Whether a scene plays (function 248).
    fn scene_playing(&mut self, scene: &FormKey) -> Result<bool, &'static str> {
        let _ = scene;
        Err("scene state is not read here")
    }
    /// Whether the action of that index of a scene is complete (550).
    fn scene_action_complete(
        &mut self,
        scene: &FormKey,
        action: u32,
    ) -> Result<bool, &'static str> {
        let _ = (scene, action);
        Err("scene state is not read here")
    }
    /// The rule the answer of function 550 rests on besides its own, when
    /// it reads the scene clock's actions (`cond_fn_550_clock_actions_v1`).
    fn action_rule(&self) -> Option<&'static str> {
        None
    }
    /// Whether function 131 is read from the scene clock
    /// (`cond_fn_131_action_percent_v1`); off, it is not evaluated, as
    /// before.
    fn action_percent_on(&self) -> bool {
        false
    }
    /// How far, in percent, the action of that index of a scene has run
    /// on the scene clock (function 131).
    fn action_percent(&mut self, scene: &FormKey, action: u32) -> Result<f32, &'static str> {
        let _ = (scene, action);
        Err("scene state is not read here")
    }
    /// How many actors of a base have died (function 84).
    fn dead_count(&mut self, base: &FormKey) -> Result<f32, &'static str> {
        let _ = base;
        Err("deaths are not kept by a run")
    }
    /// Bound of one record read.
    fn max_memory(&self) -> u64 {
        1 << 24
    }
    /// The base object a placed reference names (NAME).
    fn reference_base(&mut self, reference: &FormKey) -> Result<FormKey, &'static str> {
        let b = fo4_core::Budget::new(self.max_memory());
        crate::bind::base_of(self.store(), reference, &b)
            .map_err(|_| "the reference could not be read")?
            .ok_or("the reference has no base")
    }
    /// The base of a party of the conversation a dialogue line is tried
    /// in (`dialogue_conversation_parties_v1`): run-on 0 the speaker,
    /// run-on 1 the one spoken to. `None` outside a conversation (and for
    /// any other run-on word), where the run-on reading answers as before.
    fn party_base(&mut self, run_on: u32) -> Option<Result<FormKey, &'static str>> {
        let _ = run_on;
        None
    }
    /// Whether function 1 is read from the world model's positions
    /// (`cond_fn_1_track_distance_v1`); off, it is not evaluated, as before.
    fn distance_on(&self) -> bool {
        false
    }
    /// Whether a reference word naming the first plugin's id 0x14, which no
    /// plugin defines, stands for the player
    /// (`cond_player_reference_word_v1`); off, it names no record, as
    /// before.
    fn player_word_on(&self) -> bool {
        false
    }
    /// The reference or the player an alias of a quest holds, for function
    /// 1 (`None`: empty).
    fn alias_party(&mut self, quest: &FormKey, alias: i32) -> Result<Option<Party>, &'static str> {
        self.alias_reference(quest, alias)
            .map(|r| r.map(Party::Record))
    }
    /// The distance between two parties at the run's present second, in
    /// world units (function 1).
    fn distance(&mut self, a: &Party, b: &Party) -> Result<f32, &'static str> {
        let _ = (a, b);
        Err("positions are not read here")
    }
    /// A value the caller assumes for every condition of a function index
    /// (a caller assumption, not a rule: `quest_run --assume-cond`).
    fn assumed(&self, function: u16) -> Option<f32> {
        let _ = function;
        None
    }
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
            b"SCEN" => "a parameter names no winning SCEN",
            b"NPC_" => "a parameter names no winning NPC_",
            _ => "a parameter names no winning GLOB",
        });
    }
    Ok(k)
}

/// The placed reference a condition runs on (`cond_run_on_subject_v1`):
/// run-on 2, the reference word; run-on 5, the reference the alias of
/// the last word of the owning quest holds. Any other run-on word names a
/// subject the interpreter does not know.
pub fn subject(
    c: &Ctda,
    ctx: Context<'_>,
    file: &str,
    facts: &mut dyn Facts,
) -> Result<FormKey, &'static str> {
    if facts.switches().run_on_off {
        return Err("the run-on reading is off");
    }
    if c.flags() & flag::SWAP != 0 {
        return Err("subject and target are swapped");
    }
    match c.run_on {
        2 => {
            let k = condition::form(ctx, c.reference, file, 0)
                .map_err(|_| "the reference word is not a FormID")?
                .ok_or("the reference word is zero")?;
            match facts.store().signature(&k) {
                Some(s) if PLACED.contains(&&s) => Ok(k),
                Some(_) => Err("the reference word names no placed reference"),
                None => Err("the reference word names no record the data defines"),
            }
        }
        5 => {
            let q = facts
                .owner()
                .cloned()
                .ok_or("the list has no owning quest")?;
            facts
                .alias_reference(&q, c.param3)?
                .ok_or("the alias holds no reference")
        }
        _ => Err("the condition runs on a subject the interpreter does not know"),
    }
}

/// A reference word as a party of function 1: a placed reference, or,
/// with `cond_player_reference_word_v1`, the player for the first plugin's
/// id 0x14 that no plugin defines.
fn party_word(
    raw: u32,
    ctx: Context<'_>,
    file: &str,
    facts: &mut dyn Facts,
    used: &mut BTreeSet<&'static str>,
) -> Result<Party, &'static str> {
    let k = condition::form(ctx, raw, file, 0)
        .map_err(|_| "the reference word is not a FormID")?
        .ok_or("the reference word is zero")?;
    match facts.store().signature(&k) {
        Some(s) if PLACED.contains(&&s) => Ok(Party::Record(k)),
        Some(_) => Err("the reference word names no placed reference"),
        None => {
            let first = ctx.order.headers().first().map(|h| h.name.as_str());
            if facts.player_word_on()
                && k.object_id == PLAYER_REFERENCE_ID
                && first.is_some_and(|n| n.eq_ignore_ascii_case(&k.plugin))
            {
                used.insert(rules::COND_PLAYER_WORD);
                Ok(Party::Player)
            } else {
                Err("the reference word names no record the data defines")
            }
        }
    }
}

/// Function 1 from the world model's positions
/// (`cond_fn_1_track_distance_v1`): the subject by the run-on word (2 the
/// reference word, 5 an alias of the owning quest), the other party by
/// parameter 1 (a placed reference, or with the aliases flag an alias of
/// the owning quest).
fn distance_value(
    c: &Ctda,
    ctx: Context<'_>,
    file: &str,
    facts: &mut dyn Facts,
    used: &mut BTreeSet<&'static str>,
) -> Result<f32, &'static str> {
    let (a, b) = distance_parties(c, ctx, file, facts, used)?;
    facts.distance(&a, &b)
}

/// The two parties of a function 1 condition (see [`distance_value`]).
pub fn distance_parties(
    c: &Ctda,
    ctx: Context<'_>,
    file: &str,
    facts: &mut dyn Facts,
    used: &mut BTreeSet<&'static str>,
) -> Result<(Party, Party), &'static str> {
    if facts.switches().run_on_off {
        return Err("the run-on reading is off");
    }
    if c.flags() & (flag::SWAP | flag::PACK_DATA) != 0 {
        return Err("subject and target are swapped, or the parameters name package data");
    }
    let alias = |facts: &mut dyn Facts, id: i32| -> Result<Party, &'static str> {
        let q = facts
            .owner()
            .cloned()
            .ok_or("the list has no owning quest")?;
        facts
            .alias_party(&q, id)?
            .ok_or("the alias holds no reference")
    };
    let a = match c.run_on {
        2 => party_word(c.reference, ctx, file, facts, used)?,
        5 => alias(facts, c.param3)?,
        _ => return Err("the condition runs on a subject the interpreter does not know"),
    };
    let b = if c.flags() & flag::ALIASES != 0 {
        let id = i32::try_from(c.param1).map_err(|_| "an alias index out of range")?;
        alias(facts, id)?
    } else {
        party_word(c.param1, ctx, file, facts, used)?
    };
    Ok((a, b))
}

/// The function's value of one condition, as a number.
fn function_value(
    c: &Ctda,
    cis2: Option<&[u8]>,
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
        576 => {
            // Parameter 1: a u16 selector, then a two-character member code
            // (spec/story-manager.md); only selector 0 has a candidate.
            let p = c.param1.to_le_bytes();
            if u16::from_le_bytes([p[0], p[1]]) != 0 {
                return Err("a selector of function 576 without a candidate meaning");
            }
            let code = [p[2], p[3]];
            if !matches!(&code, b"K1" | b"L1") {
                return Err("an event member function 576 is not evaluated for");
            }
            let member = facts.event_member(code)?;
            let want = condition::form(ctx, c.param2, file, 0)
                .map_err(|_| "a parameter is not a FormID")?;
            Ok(b(member == want))
        }
        72 => {
            let want = condition::form(ctx, c.param1, file, 0)
                .map_err(|_| "a parameter is not a FormID")?
                .ok_or("a parameter is zero")?;
            let base = match facts.party_base(c.run_on) {
                Some(party) => {
                    if facts.switches().run_on_off {
                        return Err("the run-on reading is off");
                    }
                    if c.flags() & flag::SWAP != 0 {
                        return Err("subject and target are swapped");
                    }
                    party?
                }
                None => {
                    let r = subject(c, ctx, file, facts)?;
                    facts.reference_base(&r)?
                }
            };
            Ok(b(base == want))
        }
        84 => {
            let n = record(facts, ctx, c.param1, file, b"NPC_")?;
            facts.dead_count(&n)
        }
        248 => {
            let s = record(facts, ctx, c.param1, file, b"SCEN")?;
            facts.scene_playing(&s).map(b)
        }
        550 => {
            let s = record(facts, ctx, c.param1, file, b"SCEN")?;
            facts.scene_action_complete(&s, c.param2).map(b)
        }
        629 => {
            let q = record(facts, ctx, c.param1, file, b"QUST")?;
            let name = cis2.ok_or("the condition has no CIS2")?;
            facts.quest_variable(&q, &fo4_pex::index::lower(name))
        }
        _ => Err(NOT_EVALUATED),
    }
}

/// Evaluates one condition; adds the rules it relied on to `used`. The
/// condition's texts are not read: [`eval_condition`] reads them.
pub fn eval_term(
    c: &Ctda,
    ctx: Context<'_>,
    file: &str,
    facts: &mut dyn Facts,
    used: &mut BTreeSet<&'static str>,
) -> Truth {
    eval_with_text(c, None, ctx, file, facts, used)
}

/// Evaluates one condition with its texts (function 629 reads CIS2).
pub fn eval_condition(
    c: &Condition,
    ctx: Context<'_>,
    file: &str,
    facts: &mut dyn Facts,
    used: &mut BTreeSet<&'static str>,
) -> Truth {
    eval_with_text(&c.ctda, c.cis2.as_deref(), ctx, file, facts, used)
}

fn eval_with_text(
    c: &Ctda,
    cis2: Option<&[u8]>,
    ctx: Context<'_>,
    file: &str,
    facts: &mut dyn Facts,
    used: &mut BTreeSet<&'static str>,
) -> Truth {
    used.insert(rules::COND_LAYOUT);
    let f = c.function;
    if let Some(v) = facts.assumed(f) {
        // A caller assumption: the function's value is given, the
        // comparison is read as for an evaluated function.
        return compare(c, v, ctx, file, facts, used);
    }
    if f == ACTION_PERCENT && facts.action_percent_on() {
        // Function 131 read from the scene clock, off by default
        // (`cond_fn_131_action_percent_v1`).
        used.insert(rules::COND_FLAGS);
        if c.flags() & (flag::ALIASES | flag::PACK_DATA) != 0 {
            return Truth::open(f, "the parameters name aliases or package data");
        }
        used.insert(rules::COND_FN_131);
        let v = record(facts, ctx, c.param1, file, b"SCEN")
            .and_then(|s| facts.action_percent(&s, c.param2));
        return match v {
            Ok(v) => compare(c, v, ctx, file, facts, used),
            Err(why) => Truth::open(f, why),
        };
    }
    if f == DISTANCE && facts.distance_on() {
        // Function 1 read from the world model, off by default
        // (`cond_fn_1_track_distance_v1`).
        used.insert(rules::COND_FLAGS);
        used.insert(rules::COND_RUN_ON);
        used.insert(rules::COND_FN_1_TRACK);
        return match distance_value(c, ctx, file, facts, used) {
            Ok(v) => compare(c, v, ctx, file, facts, used),
            Err(why) => Truth::open(f, why),
        };
    }
    let Some(rule) = rule_of(f).filter(|_| !is_added(f) || facts.switches().on(f)) else {
        return Truth::open(f, NOT_EVALUATED);
    };
    used.insert(rules::COND_FLAGS);
    if c.flags() & (flag::ALIASES | flag::PACK_DATA) != 0 {
        return Truth::open(f, "the parameters name aliases or package data");
    }
    used.insert(rule);
    if f == 72 {
        used.insert(rules::COND_RUN_ON);
    }
    if f == 550 {
        if let Some(r) = facts.action_rule() {
            used.insert(r);
        }
    }
    let v = match function_value(c, cis2, ctx, file, facts) {
        Ok(v) => v,
        Err(why) => return Truth::open(f, why),
    };
    compare(c, v, ctx, file, facts, used)
}

/// Compares a function's value `v` with the condition's comparison value
/// under its operator code.
fn compare(
    c: &Ctda,
    v: f32,
    ctx: Context<'_>,
    file: &str,
    facts: &mut dyn Facts,
    used: &mut BTreeSet<&'static str>,
) -> Truth {
    let f = c.function;
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
        .map(|c| eval_condition(c, ctx, file, facts, used))
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
    switches: Switches,
    /// The compiled scripts, for the initial values of quest variables
    /// (function 629); `None` leaves them undetermined.
    scripts: Option<Box<dyn Library + 's>>,
    owner: Option<FormKey>,
}
impl<'s> StaticFacts<'s> {
    pub fn new_game(store: &'s Store, max_memory: u64) -> Self {
        StaticFacts {
            store,
            max_memory,
            running: BTreeMap::new(),
            switches: Switches::all(),
            scripts: None,
            owner: None,
        }
    }
    /// The same facts with these evaluator switches.
    pub fn with_switches(mut self, s: Switches) -> Self {
        self.switches = s;
        self
    }
    /// The same facts reading quest variables' initial values from these
    /// compiled scripts.
    pub fn with_scripts(mut self, lib: Box<dyn Library + 's>) -> Self {
        self.scripts = Some(lib);
        self
    }
    /// The quest whose aliases run-on 5 names in the next lists.
    pub fn set_owner(&mut self, quest: Option<FormKey>) {
        self.owner = quest;
    }
}
impl Facts for StaticFacts<'_> {
    fn store(&self) -> &Store {
        self.store
    }
    fn switches(&self) -> &Switches {
        &self.switches
    }
    fn max_memory(&self) -> u64 {
        self.max_memory
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
    fn owner(&self) -> Option<&FormKey> {
        self.owner.as_ref()
    }
    /// A quest that runs at the start of a new game holds the forced
    /// reference of an alias that has one (`quest_alias_forced_reference_v1`);
    /// a quest that does not run holds none.
    fn alias_reference(
        &mut self,
        quest: &FormKey,
        alias: i32,
    ) -> Result<Option<FormKey>, &'static str> {
        if !self.quest_running(quest)? {
            return Ok(None);
        }
        let b = fo4_core::Budget::new(self.max_memory);
        let i = crate::quest::QuestInfo::load(self.store, quest, &b)
            .ok()
            .flatten()
            .ok_or("the quest record could not be read")?;
        let forced = u32::try_from(alias)
            .ok()
            .and_then(|a| i.reference_aliases.get(&a))
            .ok_or("no reference alias of that index")?;
        forced
            .clone()
            .map(Some)
            .ok_or("the alias has no forced reference")
    }
    fn quest_variable(&mut self, quest: &FormKey, name: &str) -> Result<f32, &'static str> {
        let lib = self
            .scripts
            .as_deref_mut()
            .ok_or("compiled scripts are not read here")?;
        let b = fo4_core::Budget::new(self.max_memory);
        crate::vars::quest_variable_initial(self.store, lib, quest, name, &b)
    }
    /// No scene plays at the start of a new game (`scene_start_v1`).
    fn scene_playing(&mut self, _: &FormKey) -> Result<bool, &'static str> {
        Ok(false)
    }
    /// No scene has played, so no action is complete
    /// (`cond_fn_550_scene_action_complete_v1`).
    fn scene_action_complete(&mut self, _: &FormKey, _: u32) -> Result<bool, &'static str> {
        Ok(false)
    }
    /// No actor has died (`cond_fn_84_dead_count_v1`).
    fn dead_count(&mut self, _: &FormKey) -> Result<f32, &'static str> {
        Ok(0.0)
    }
}
