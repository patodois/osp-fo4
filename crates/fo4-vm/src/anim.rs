//! Animation events (`spec/vm.md`, Animation events): registrations made by
//! `ScriptObject.RegisterForAnimationEvent`, delivered as
//! `OnAnimationEvent(sender, name)` to the registered instances where the
//! run raises the event, and the graphs that can raise them from the data:
//! a reference's base model plays its behaviour graph
//! (`fo4_scene::anim_graph`), `ObjectReference.PlayAnimation` sends the
//! graph an event, a transition enters a state whose Gamebryo sequence
//! plays, and the sequence's text keys are raised at their times.
//!
//! Every behaviour is a pending rule, each behind its own switch, all off
//! by default: `rules::ANIM_EVENTS` (registrations and delivery),
//! `rules::ANIM_EVENT_CASE` (names compared ignoring case),
//! `rules::ANIM_GRAPH` (the model's graph and its transitions) and
//! `rules::ANIM_GRAPH_KEYS` (text keys raised). With every switch off the
//! natives are recorded as any other native, as before. Nothing here is
//! measured in the running game.
use crate::{
    bind,
    host::{NativeCall, Reply},
    quest, rules,
    world::{InstId, ObjId, Val, World},
    Outcome, Vm, VmError,
};
use fo4_core::Budget;
use fo4_plugin::{typed::Store, FormKey};
use fo4_scene::anim_graph::ModelGraph;
use std::{collections::BTreeMap, rc::Rc};

/// Reads the graph of a model text (MODL), for a caller that mounts the
/// archives (`fo4_scene::anim_graph::model_graph`).
pub trait GraphSource {
    fn graph(&mut self, model: &str) -> Result<Rc<ModelGraph>, String>;
}

/// One registration: the instance that registered, the sender it listens
/// to and the event name as the registering call stored it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub instance: InstId,
    pub sender: ObjId,
    pub event: String,
}

/// A reference's graph in the run: the state it is in and the generation
/// of that entry (keys of an earlier entry are dropped).
#[derive(Debug, Clone)]
pub struct GraphRun {
    pub graph: Rc<ModelGraph>,
    pub state: i32,
    pub generation: u64,
    /// Why the graph stopped being followed (a transition the reading does
    /// not take), if it did.
    pub open: Option<String>,
}

/// A text key due on the real-time clock.
#[derive(Debug, Clone, PartialEq)]
pub struct Due {
    pub t: f64,
    pub sender: ObjId,
    pub text: String,
    pub generation: u64,
}

/// The switches, registrations and graph runs of a run (kept in
/// [`World`]).
#[derive(Debug, Default)]
pub struct Anims {
    /// Keep registrations and deliver animation events the run raises
    /// (`rules::ANIM_EVENTS`, `quest_run --anim-events`).
    pub on: bool,
    /// Compare event names ignoring ASCII case, the handler receiving the
    /// registration's spelling (`rules::ANIM_EVENT_CASE`,
    /// `--anim-event-case`); off: names must be equal.
    pub ignore_case: bool,
    /// Follow references' graphs through `PlayAnimation`
    /// (`rules::ANIM_GRAPH`, `--anim-graphs`).
    pub graphs: bool,
    /// Raise the text keys of the sequences the graphs play
    /// (`rules::ANIM_GRAPH_KEYS`, `--anim-graph-keys`).
    pub keys: bool,
    pub registrations: Vec<Registration>,
    /// The value `RegisterForAnimationEvent` answers where the
    /// registration is made: a caller assumption
    /// (`quest_run --assume-anim-registration true|false`); `None` by
    /// default: undetermined, as before.
    pub assume_registration: Option<bool>,
    pub runs: BTreeMap<ObjId, GraphRun>,
    /// References whose graph could not be read, with why.
    pub refused: BTreeMap<ObjId, String>,
    /// Text keys due, in scheduling order.
    pub due: Vec<Due>,
    /// (real-time second, what happened), in order.
    pub log: Vec<(f64, String)>,
    pub counts: BTreeMap<String, u64>,
    /// Rules relied on by observations inside a native call, taken by the
    /// host after the call.
    pub used: Vec<&'static str>,
}
impl Anims {
    pub fn count(&mut self, k: impl Into<String>) {
        *self.counts.entry(k.into()).or_default() += 1;
    }
    fn same(&self, a: &str, b: &str) -> bool {
        a == b || (self.ignore_case && a.eq_ignore_ascii_case(b))
    }
    fn using(&mut self, r: &'static str) {
        if !self.used.contains(&r) {
            self.used.push(r);
        }
    }
}

/// The natives of registration.
pub const NATIVES: [&str; 2] = [
    "scriptobject.registerforanimationevent",
    "scriptobject.unregisterforanimationevent",
];
/// The native whose event drives a graph.
pub const PLAY: &str = "objectreference.playanimation";

/// The registration natives when [`Anims::on`]: the registration is kept
/// and the call answers as before (a recorded, undetermined result for
/// `RegisterForAnimationEvent`, none for the other).
pub fn native(c: &NativeCall, w: &mut World) -> Option<Reply> {
    let full = c.full_name();
    if !w.anims.on || !NATIVES.contains(&full.as_str()) {
        return None;
    }
    let sender = match c.args.first() {
        Some(Val::Object(o)) => Some(*o),
        _ => None,
    };
    let name = match c.args.get(1) {
        Some(Val::Str(s)) => Some(s.clone()),
        _ => None,
    };
    let register = full.ends_with(".registerforanimationevent");
    let mut made = false;
    match (c.instance, sender, name) {
        (Some(i), Some(s), Some(e)) => {
            made = register;
            w.anims.using(rules::ANIM_EVENTS);
            if register {
                w.anims.count("registrations\tmade");
                let r = Registration {
                    instance: i,
                    sender: s,
                    event: e,
                };
                if !w.anims.registrations.contains(&r) {
                    w.anims.registrations.push(r);
                }
            } else {
                w.anims.count("registrations\tremoved");
                let before = w.anims.registrations.len();
                let ignore = w.anims.ignore_case;
                w.anims.registrations.retain(|r| {
                    !(r.instance == i
                        && r.sender == s
                        && (r.event == e || (ignore && r.event.eq_ignore_ascii_case(&e))))
                });
                if before == w.anims.registrations.len() {
                    w.anims.count("registrations\tremoval named none");
                }
            }
        }
        _ => w.anims.count(format!(
            "registrations\t{}: the instance, the sender or the name is undetermined",
            if register { "not made" } else { "not removed" }
        )),
    }
    Some(if register {
        match w.anims.assume_registration.filter(|_| made) {
            Some(v) => {
                w.anims.count(format!(
                    "registrations\tanswered by the caller's assumption: {v}"
                ));
                Reply::Value(Val::Bool(v))
            }
            None => Reply::Undetermined,
        }
    } else {
        Reply::Value(Val::None)
    })
}

/// The model text (MODL) of a placed reference's base.
fn base_model(store: &Store, k: &FormKey, b: &Budget) -> Result<String, String> {
    let base = bind::base_of(store, k, b)
        .map_err(|e| e.to_string())?
        .ok_or("the reference has no base")?;
    let r = store
        .raw(&base, b)
        .map_err(|e| e.to_string())?
        .ok_or("the base is not a winning record")?;
    let d = r
        .subrecords
        .iter()
        .find(|s| s.signature == *b"MODL")
        .map(|s| r.data(s))
        .ok_or("the base has no model")?;
    let d = d.strip_suffix(&[0]).unwrap_or(d);
    Ok(String::from_utf8_lossy(d).into_owned())
}

/// Enters a state at `now`: a new generation, and with
/// [`Anims::keys`] the text keys of its sequence scheduled.
fn enter(w: &mut World, o: ObjId, to: i32, now: f64) {
    let Some(run) = w.anims.runs.get_mut(&o) else {
        return;
    };
    run.state = to;
    run.generation += 1;
    let (g, generation) = (run.graph.clone(), run.generation);
    let label = w.objects[o].label.clone();
    let Some(state) = g.machine.state(to) else {
        w.anims
            .log
            .push((now, format!("{label}: state {to} is not in the graph")));
        return;
    };
    w.anims.log.push((
        now,
        format!(
            "{label}: entered state {} ({})",
            state.name,
            state.sequence.as_deref().unwrap_or(&state.generator)
        ),
    ));
    if !w.anims.keys {
        return;
    }
    let Some(seq) = state.sequence.as_deref().and_then(|s| g.sequence(s)) else {
        w.anims.count(if state.sequence.is_some() {
            "graph states entered\tthe sequence is not in the model"
        } else {
            "graph states entered\tthe generator is not a sequence generator"
        });
        return;
    };
    w.anims.using(rules::ANIM_GRAPH_KEYS);
    w.anims.count(format!(
        "graph states entered\ta sequence of cycle type {}",
        seq.cycle
    ));
    let speed = if seq.frequency.is_finite() && seq.frequency > 0.0 {
        f64::from(seq.frequency)
    } else {
        w.anims
            .count("text keys not scheduled\tthe sequence's frequency is not positive");
        return;
    };
    for (t, text) in &seq.keys {
        let raised = g.machine.events.iter().any(|e| w.anims.same(e, text));
        if !raised {
            w.anims
                .count("text keys not scheduled\tthe text is no event name of the graph");
            continue;
        }
        let at = now + (f64::from(*t) - f64::from(seq.start)).max(0.0) / speed;
        w.anims.due.push(Due {
            t: at,
            sender: o,
            text: text.clone(),
            generation,
        });
    }
}

/// Starts following a reference's graph (`rules::ANIM_GRAPH`): in the
/// root machine's start state, none of that state's keys raised (when it
/// was entered is not known).
pub fn begin(w: &mut World, o: ObjId, g: Rc<ModelGraph>, now: f64) {
    let start = g.machine.start;
    w.anims.using(rules::ANIM_GRAPH);
    w.anims.count("graphs read");
    let label = w.objects[o].label.clone();
    w.anims.log.push((
        now,
        format!("{label}: graph read, start state {start} (its keys not raised)"),
    ));
    w.anims.runs.insert(
        o,
        GraphRun {
            graph: g,
            state: start,
            generation: 0,
            open: None,
        },
    );
}

/// Sends a graph event to a reference's graph (`rules::ANIM_GRAPH`): the
/// current state's transition of that event, else a wildcard transition.
pub fn graph_event(w: &mut World, o: ObjId, event: &str, now: f64) {
    let Some(run) = w.anims.runs.get(&o) else {
        return;
    };
    if run.open.is_some() {
        w.anims
            .count("graph events not followed\tthe graph is undetermined");
        return;
    }
    let g = run.graph.clone();
    let state = run.state;
    let label = w.objects[o].label.clone();
    let Some(index) = g.machine.events.iter().position(|e| w.anims.same(e, event)) else {
        w.anims
            .count("graph events not taken\tthe name is no event of the graph");
        w.anims.log.push((
            now,
            format!("{label}: event {event} is no event of the graph"),
        ));
        return;
    };
    let local = g
        .machine
        .state(state)
        .map(|s| s.transitions.clone())
        .unwrap_or_default();
    let found = local
        .iter()
        .chain(g.machine.wildcards.iter())
        .find(|t| t.event == index as i32)
        .copied();
    let Some(t) = found else {
        w.anims
            .count("graph events not taken\tno transition of the state or wildcard");
        w.anims.log.push((
            now,
            format!("{label}: event {event} takes no transition from state {state}"),
        ));
        return;
    };
    w.anims.using(rules::ANIM_GRAPH);
    if t.condition.is_some() || g.machine.state(t.to_state).is_none() {
        let why = if t.condition.is_some() {
            "a transition with a condition"
        } else {
            "a transition to a state the machine does not hold"
        };
        w.anims.count(format!("graph events not followed\t{why}"));
        w.anims.log.push((now, format!("{label}: {event}: {why}")));
        if let Some(r) = w.anims.runs.get_mut(&o) {
            r.open = Some(why.into());
        }
        return;
    }
    w.anims.count("graph events taken");
    enter(w, o, t.to_state, now);
}

/// `PlayAnimation(event)` on a placed reference, observed before the host
/// answers it as before (`rules::ANIM_GRAPH`): the reference's graph is
/// read when first needed, starts in its start state with none of that
/// state's keys raised, and takes the event.
pub fn observe_play(
    store: &Store,
    max: u64,
    graphs: &mut dyn GraphSource,
    c: &NativeCall,
    w: &mut World,
) {
    if !w.anims.graphs || c.full_name() != PLAY {
        return;
    }
    let (Some(Val::Object(o)), Some(Val::Str(event))) = (&c.receiver, c.args.first()) else {
        w.anims
            .count("PlayAnimation not followed\tthe reference or the event is undetermined");
        return;
    };
    let (o, event) = (*o, event.clone());
    let now = w.now[crate::world::Clock::Real as usize];
    if !w.anims.runs.contains_key(&o) {
        if w.anims.refused.contains_key(&o) {
            w.anims
                .count("PlayAnimation not followed\tthe reference's graph was refused");
            return;
        }
        let b = Budget::new(max);
        let key = w.objects[o].key.clone();
        let got = key
            .as_deref()
            .and_then(crate::data::key_record)
            .ok_or_else(|| "the reference is no record".to_string())
            .and_then(|k| base_model(store, &k, &b))
            .and_then(|m| graphs.graph(&m));
        match got {
            Ok(g) => begin(w, o, g, now),
            Err(why) => {
                w.anims.count("graphs refused");
                let label = w.objects[o].label.clone();
                w.anims
                    .log
                    .push((now, format!("{label}: graph refused: {why}")));
                w.anims.refused.insert(o, why);
                return;
            }
        }
    }
    w.anims.count("PlayAnimation followed");
    graph_event(w, o, &event, now);
}

/// The earliest second a text key is due, when one is.
pub fn next_due(w: &World) -> Option<f64> {
    w.anims.due.iter().map(|d| d.t).min_by(f64::total_cmp)
}

/// One animation event the run raised.
#[derive(Debug, Clone)]
pub struct Raised {
    pub t: f64,
    pub sender: ObjId,
    pub name: String,
    /// What raised it: a text key, or the caller.
    pub from: &'static str,
    /// (script, outcome) per registered instance.
    pub runs: Vec<(String, Outcome)>,
}

/// Delivers an animation event of `sender` to the instances registered for
/// it, in registration order (`rules::ANIM_EVENTS`): the handler
/// `OnAnimationEvent(sender, name)` with the name as registered.
pub fn deliver(vm: &mut Vm<'_>, sender: ObjId, name: &str) -> Vec<(String, Outcome)> {
    let mut out = Vec::new();
    if !vm.world.anims.on {
        return out;
    }
    let regs: Vec<Registration> = vm
        .world
        .anims
        .registrations
        .iter()
        .filter(|r| r.sender == sender && vm.world.anims.same(&r.event, name))
        .cloned()
        .collect();
    if regs.iter().any(|r| r.event != name) {
        vm.rules.insert(rules::ANIM_EVENT_CASE);
    }
    for r in regs {
        vm.rules.insert(rules::ANIM_EVENTS);
        let script = vm.world.instances[r.instance].script.clone();
        match vm.find_handler(r.instance, "onanimationevent") {
            Ok(Some(f)) if f.params.len() == 2 => {
                let args = vec![Val::Object(sender), Val::Str(r.event.clone())];
                let o = vm.deliver(r.instance, "onanimationevent", args);
                vm.world.anims.count("deliveries\ta handler ran");
                out.push((script, o));
            }
            Ok(Some(_)) => out.push((
                script,
                Outcome::Failed(VmError::BadArgument {
                    site: None,
                    detail: "onanimationevent does not take a sender and a name".into(),
                }),
            )),
            Ok(None) => vm
                .world
                .anims
                .count("deliveries\tthe registered instance has no handler"),
            Err(e) => out.push((script, Outcome::Failed(e))),
        }
    }
    out
}

/// Raises the text keys due by the run's present second, earliest first:
/// each is sent to its reference's graph and delivered to the registered
/// instances; the jobs each delivery brings run before the next. A key of
/// an earlier entry of its state is dropped.
pub fn settle(
    vm: &mut Vm<'_>,
    store: &Store,
    max_jobs: usize,
    b: &Budget,
) -> (Vec<Raised>, Vec<quest::JobRun>) {
    let mut raised = Vec::new();
    let mut runs = Vec::new();
    let now = vm.world.now[crate::world::Clock::Real as usize];
    for _ in 0..10_000 {
        let Some(i) = vm
            .world
            .anims
            .due
            .iter()
            .enumerate()
            .filter(|(_, d)| d.t <= now)
            .min_by(|a, b| a.1.t.total_cmp(&b.1.t))
            .map(|(i, _)| i)
        else {
            break;
        };
        let d = vm.world.anims.due.remove(i);
        let current = vm
            .world
            .anims
            .runs
            .get(&d.sender)
            .is_some_and(|r| r.generation == d.generation && r.open.is_none());
        if !current {
            vm.world
                .anims
                .count("text keys dropped\tthe state was left first");
            continue;
        }
        vm.rules.insert(rules::ANIM_GRAPH_KEYS);
        let label = vm.world.objects[d.sender].label.clone();
        vm.world
            .anims
            .log
            .push((d.t, format!("{label}: text key {} raised", d.text)));
        vm.world.anims.count("text keys raised");
        let out = deliver(vm, d.sender, &d.text);
        runs.extend(quest::run_jobs(vm, store, max_jobs, b));
        graph_event(&mut vm.world, d.sender, &d.text, d.t);
        for r in std::mem::take(&mut vm.world.anims.used) {
            vm.rules.insert(r);
        }
        raised.push(Raised {
            t: d.t,
            sender: d.sender,
            name: d.text,
            from: "a text key of the sequence its graph plays",
            runs: out,
        });
    }
    (raised, runs)
}
