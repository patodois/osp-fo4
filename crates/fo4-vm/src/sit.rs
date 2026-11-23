//! Furniture use (`spec/vm.md`, Furniture use): who sits in which
//! furniture reference in the run, the sit and get-up events that use
//! raises, and the native that asks whether a furniture is in use.
//!
//! The interpreter has no actor who walks to a chair. A sit happens only
//! where a caller assumes it (`quest_run --assume-sit`,
//! `--assume-get-up`) or, under `rules::SIT_ACTIVATE` (off by default),
//! where a script activates a furniture reference for an actor with
//! default processing only. Every behaviour is a pending rule:
//! `rules::SIT_EVENTS` (who receives `OnSit`, `OnGetUp` and
//! `OnExitFurniture`, and with what), `rules::SIT_ACTIVATE` and
//! `rules::SIT_IN_USE` (`IsFurnitureInUse` answered from the run's
//! sitters), `rules::SIT_ACTIVATE_EVENTS` (a script's activation with
//! events sits too), `rules::SIT_ACTIVATE_GET_UP` (a script's activation
//! of the furniture the actor sits in gets it up) and
//! `rules::FURNITURE_GRAPH_EVENTS` (those sits and get-ups send the
//! furniture's behaviour graph an event the caller names) and
//! `rules::SIT_STATE` (`GetSitState` answered from the run's sitters), each
//! behind its own switch. Nothing is measured in the running game; no idle or package
//! is modelled.
use crate::{
    bind,
    data::key_record,
    host::{NativeCall, Reply},
    player::{self, Delivery},
    rules,
    world::{ObjId, Val, World},
};
use fo4_core::Budget;
use fo4_plugin::typed::Store;
use std::collections::BTreeMap;

/// The run's furniture state (kept in [`World`]).
#[derive(Debug, Default)]
pub struct Sits {
    /// A script's `Activate(actor, true)` of a furniture reference makes
    /// that actor sit in it (`rules::SIT_ACTIVATE`,
    /// `quest_run --activate-furniture-sits`).
    pub activate: bool,
    /// `IsFurnitureInUse` answered from the run's sitters
    /// (`rules::SIT_IN_USE`, `quest_run --furniture-in-use`).
    pub in_use: bool,
    /// A script's `Activate(actor, false)` of a furniture reference the
    /// actor is not in makes that actor sit in it
    /// (`rules::SIT_ACTIVATE_EVENTS`, `--activate-furniture-events`).
    pub activate_events: bool,
    /// A script's `Activate(actor, true|false)` of the furniture the run
    /// has the actor sitting in makes it get up (`rules::SIT_ACTIVATE_GET_UP`,
    /// `--activate-furniture-get-up`).
    pub activate_get_up: bool,
    /// The graph events a sit and a get-up that a script's activation brings
    /// send to the furniture reference's behaviour graph: (sit, get-up), the
    /// caller's names (`rules::FURNITURE_GRAPH_EVENTS`,
    /// `--furniture-graph-events SIT GET_UP`, with the animation graph
    /// reading); `None`: no graph event is sent.
    pub graph_events: Option<(String, String)>,
    /// `Actor.GetSitState` on the player's handle or a placed actor answered
    /// from the run's sitters: [`SIT_STATE_SITTING`] while the run has it
    /// sitting in a furniture, 0 otherwise (`rules::SIT_STATE`,
    /// `quest_run --sit-state`); off: recorded, undetermined, as before.
    pub sit_state: bool,
    /// Graph events to send (furniture, event name), taken by the host
    /// within the call that brought them (`crate::anim::furniture_event`).
    pub graph_sends: Vec<(ObjId, String)>,
    /// Who sits where: the sitter (the player's handle or a placed actor)
    /// and the furniture reference.
    pub sitting: BTreeMap<ObjId, ObjId>,
    /// Sits a native brought, delivered after the call ([`settle`]):
    /// (sitter, furniture).
    pub pending: Vec<(ObjId, ObjId)>,
    /// Get-ups a native brought, delivered after the call before the sits
    /// ([`settle`]): (sitter, furniture).
    pub pending_get_ups: Vec<(ObjId, ObjId)>,
    /// (real-time second, what happened), in order.
    pub log: Vec<(f64, String)>,
    pub counts: BTreeMap<String, u64>,
    /// Rules relied on inside a native call, taken by the host after it.
    pub used: Vec<&'static str>,
}
impl Sits {
    pub fn count(&mut self, k: impl Into<String>) {
        *self.counts.entry(k.into()).or_default() += 1;
    }
    fn using(&mut self, r: &'static str) {
        if !self.used.contains(&r) {
            self.used.push(r);
        }
    }
    /// Whether a native brought sits or get-ups not yet delivered.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty() || !self.pending_get_ups.is_empty()
    }
    /// Notes the graph event of a sit (`true`) or a get-up under
    /// `rules::FURNITURE_GRAPH_EVENTS`, when the caller named the events.
    fn graph_event(&mut self, furniture: ObjId, sitting: bool) {
        let Some((sit, up)) = &self.graph_events else {
            return;
        };
        let name = if sitting { sit.clone() } else { up.clone() };
        self.graph_sends.push((furniture, name));
        self.count(if sitting {
            "furniture graph events	noted for a sit"
        } else {
            "furniture graph events	noted for a get-up"
        });
    }
}

/// The natives the furniture model reads or answers.
pub const NATIVES: [&str; 3] = [
    "objectreference.activate",
    "objectreference.isfurnitureinuse",
    "actor.getsitstate",
];

/// The value `GetSitState` answers under `rules::SIT_STATE` for an actor the
/// run has sitting: the documented "sitting" value (prior knowledge; the
/// scripts the forced runs reach only compare the answer with 0).
pub const SIT_STATE_SITTING: i32 = 3;

/// Whether the object is a placed reference whose base is a furniture
/// (FURN).
fn is_furniture(store: &Store, max: u64, w: &World, o: ObjId) -> bool {
    let Some(k) = w.objects[o].key.as_deref().and_then(key_record) else {
        return false;
    };
    if store.signature(&k) != Some(*b"REFR") {
        return false;
    }
    let b = Budget::new(max);
    matches!(bind::base_of(store, &k, &b), Ok(Some(base)) if store.signature(&base) == Some(*b"FURN"))
}

/// Whether the object can sit: the player's handle or a placed actor.
fn is_sitter(store: &Store, w: &World, o: ObjId) -> bool {
    Some(o) == w.player
        || w.objects[o]
            .key
            .as_deref()
            .and_then(key_record)
            .is_some_and(|k| store.signature(&k) == Some(*b"ACHR"))
}

/// The furniture natives: a scripted `Activate` of a furniture reference
/// is noted as a sit under [`Sits::activate`] (the call itself answers as
/// before: `None` is returned for it), and `IsFurnitureInUse` is answered
/// under [`Sits::in_use`].
pub fn native(store: &Store, max: u64, c: &NativeCall, w: &mut World) -> Option<Reply> {
    let full = c.full_name();
    if !NATIVES.contains(&full.as_str()) {
        return None;
    }
    let Some(Val::Object(f)) = c.receiver.clone() else {
        return None;
    };
    if full == "actor.getsitstate" {
        if !w.sits.sit_state || !is_sitter(store, w, f) {
            return None;
        }
        w.sits.using(rules::SIT_STATE);
        let sitting = w.sits.sitting.contains_key(&f);
        w.sits.count(if sitting {
            "GetSitState answered: sitting"
        } else {
            "GetSitState answered: not sitting"
        });
        return Some(Reply::Value(Val::Int(if sitting {
            SIT_STATE_SITTING
        } else {
            0
        })));
    }
    if full == "objectreference.isfurnitureinuse" {
        if !w.sits.in_use || !is_furniture(store, max, w, f) {
            return None;
        }
        w.sits.using(rules::SIT_IN_USE);
        w.sits.count("IsFurnitureInUse answered");
        let used = w.sits.sitting.values().any(|x| *x == f);
        return Some(Reply::Value(Val::Bool(used)));
    }
    let any = w.sits.activate || w.sits.activate_events || w.sits.activate_get_up;
    if !any || !is_furniture(store, max, w, f) {
        return None;
    }
    let sitter = match c.args.first() {
        Some(Val::Object(a)) if is_sitter(store, w, *a) => *a,
        _ => {
            w.sits
                .count("furniture activations not read as a sit: the activator is not the player or a placed actor");
            return None;
        }
    };
    // The actor already sits in that furniture: under the get-up reading it
    // gets up; off, the activation is read as before.
    let inside = w.sits.sitting.get(&sitter) == Some(&f);
    if inside && w.sits.activate_get_up {
        match c.args.get(1) {
            Some(Val::Bool(_)) => {
                w.sits.using(rules::SIT_ACTIVATE_GET_UP);
                w.sits.count(
                    "furniture activations read as a get-up: the activator sits in that furniture",
                );
                w.sits.pending_get_ups.push((sitter, f));
                if w.sits.graph_events.is_some() {
                    w.sits.using(rules::FURNITURE_GRAPH_EVENTS);
                    w.sits.graph_event(f, false);
                }
            }
            _ => w.sits.count(
                "furniture activations not read as a get-up: whether default processing only is undetermined",
            ),
        }
        return None;
    }
    let sit = match c.args.get(1) {
        Some(Val::Bool(true)) if w.sits.activate => {
            w.sits.using(rules::SIT_ACTIVATE);
            w.sits
                .count("furniture activations read as a sit: default processing only");
            true
        }
        Some(Val::Bool(false)) if w.sits.activate_events => {
            w.sits.using(rules::SIT_ACTIVATE_EVENTS);
            w.sits.count(
                "furniture activations read as a sit: with events (the furniture's OnActivate is not delivered)",
            );
            true
        }
        Some(Val::Bool(true)) => {
            w.sits.count(
                "furniture activations not read as a sit: default processing only (that reading is off)",
            );
            false
        }
        Some(Val::Bool(false)) => {
            w.sits.count(
                "furniture activations not read as a sit: with events (the furniture's activation is not modelled)",
            );
            false
        }
        _ => {
            w.sits.count(
                "furniture activations not read as a sit: whether default processing only is undetermined",
            );
            false
        }
    };
    if sit {
        w.sits.pending.push((sitter, f));
        if w.sits.graph_events.is_some() {
            w.sits.using(rules::FURNITURE_GRAPH_EVENTS);
            w.sits.graph_event(f, true);
        }
    }
    None
}

/// One sit or get-up delivered: what happened and the handler runs.
#[derive(Debug)]
pub struct Raised {
    pub t: f64,
    pub what: String,
    pub runs: Vec<Delivery>,
}

/// The sitter sits in the furniture (`rules::SIT_EVENTS`): `OnSit(furniture)`
/// on the sitter's scripts, the aliases holding it (the player's aliases for
/// the player) and the instances registered for its remote event. A sitter
/// the run has in another furniture leaves it without an event (counted).
pub fn sit(
    vm: &mut crate::Vm<'_>,
    store: &Store,
    sitter: ObjId,
    furniture: ObjId,
    b: &Budget,
) -> Result<Vec<Raised>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    if let Some(old) = vm.world.sits.sitting.get(&sitter).copied() {
        if old == furniture {
            vm.world.sits.count("sits\talready in that furniture");
            return Ok(out);
        }
        // When the sitter left the earlier furniture is not known (no
        // get-up was assumed or raised): it is left without an event.
        let _ = old;
        vm.world.sits.count(
            "sits\tthe sitter's earlier furniture left without an event (when is not known)",
        );
    }
    vm.rules.insert(rules::SIT_EVENTS);
    vm.world.sits.sitting.insert(sitter, furniture);
    vm.world.sits.count("sits");
    let t = vm.world.now[crate::world::Clock::Real as usize];
    let what = format!(
        "{} sits in {}",
        vm.world.objects[sitter].label, vm.world.objects[furniture].label
    );
    vm.world.sits.log.push((t, what.clone()));
    let (runs, _) =
        player::deliver_event(vm, store, sitter, "onsit", vec![Val::Object(furniture)], b)?;
    out.push(Raised { t, what, runs });
    Ok(out)
}

/// The sitter gets up from the furniture (`rules::SIT_EVENTS`):
/// `OnGetUp(furniture)` on the sitter's receivers as [`sit`] names them,
/// then `OnExitFurniture(sitter)` on the furniture's own scripts, the
/// aliases holding it and its registered instances.
pub fn get_up(
    vm: &mut crate::Vm<'_>,
    store: &Store,
    sitter: ObjId,
    furniture: ObjId,
    b: &Budget,
) -> Result<Vec<Raised>, Box<dyn std::error::Error>> {
    vm.rules.insert(rules::SIT_EVENTS);
    if vm.world.sits.sitting.get(&sitter) == Some(&furniture) {
        vm.world.sits.sitting.remove(&sitter);
    } else {
        vm.world
            .sits
            .count("get-ups\tthe run did not have the sitter in that furniture");
    }
    vm.world.sits.count("get-ups");
    let t = vm.world.now[crate::world::Clock::Real as usize];
    let what = format!(
        "{} gets up from {}",
        vm.world.objects[sitter].label, vm.world.objects[furniture].label
    );
    vm.world.sits.log.push((t, what.clone()));
    let (mut runs, _) = player::deliver_event(
        vm,
        store,
        sitter,
        "ongetup",
        vec![Val::Object(furniture)],
        b,
    )?;
    let (exit, _) = player::deliver_event(
        vm,
        store,
        furniture,
        "onexitfurniture",
        vec![Val::Object(sitter)],
        b,
    )?;
    runs.extend(exit);
    Ok(vec![Raised { t, what, runs }])
}

/// Delivers the sits the natives brought ([`Sits::pending`]), in order, and
/// runs the jobs each brings.
pub fn settle(
    vm: &mut crate::Vm<'_>,
    store: &Store,
    max_jobs: usize,
    b: &Budget,
) -> (Vec<Raised>, Vec<crate::quest::JobRun>) {
    let mut raised = Vec::new();
    let mut runs = Vec::new();
    for _ in 0..16 {
        let get_ups = std::mem::take(&mut vm.world.sits.pending_get_ups);
        for (sitter, f) in &get_ups {
            match get_up(vm, store, *sitter, *f, b) {
                Ok(r) => raised.extend(r),
                Err(e) => vm.world.sits.count(format!("get-ups\tnot delivered: {e}")),
            }
            runs.extend(crate::quest::run_jobs(vm, store, max_jobs, b));
        }
        let pending = std::mem::take(&mut vm.world.sits.pending);
        if pending.is_empty() && get_ups.is_empty() {
            break;
        }
        for (sitter, f) in pending {
            match sit(vm, store, sitter, f, b) {
                Ok(r) => raised.extend(r),
                Err(e) => vm.world.sits.count(format!("sits\tnot delivered: {e}")),
            }
            runs.extend(crate::quest::run_jobs(vm, store, max_jobs, b));
        }
    }
    (raised, runs)
}
