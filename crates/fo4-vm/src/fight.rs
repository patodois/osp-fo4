//! The first fight's events (`spec/vm.md`, The first fight): hits, combat
//! state changes and deaths, raised only where a caller assumes them
//! (`quest_run --assume-hit`, `--assume-combat-state`, `--assume-death`),
//! and the natives that read them back.
//!
//! The interpreter has no fight: no actor aims, nothing loses health and
//! no one decides to attack. What the run models is who receives the
//! events a fight raises and what the natives the receivers call answer
//! afterwards. Every behaviour is a pending rule behind its own switch,
//! off by default: `rules::HIT_EVENTS` (hit event registrations kept and a
//! hit delivered to the registered instances, once per registration),
//! `rules::HIT_INIT_REGISTRATION` (a registration an init handler the run
//! never delivers would make, read as made), `rules::COMBAT_STATE_EVENTS`
//! and `rules::DEATH_EVENTS` (the actor's own scripts, the aliases holding
//! it and the instances registered for its remote event receive
//! `OnCombatStateChanged` and `OnDying` then `OnDeath`),
//! `rules::DEAD_STATE` (`IsDead` on an actor the run killed) and
//! `rules::COMBAT_STATE_NATIVES` (`IsInCombat` and `GetCombatState` from
//! the run's assumed combat states). Active magic effects are not
//! modelled: no effect receives an event. Nothing is measured in the
//! running game.
use crate::{
    data::key_record,
    host::{NativeCall, Reply},
    player::{self, Delivery, Receiver},
    rules,
    world::{InstId, ObjId, Val, World},
    Lit, Outcome, Vm, VmError,
};
use fo4_core::Budget;
use fo4_plugin::typed::Store;
use std::collections::{BTreeMap, BTreeSet};

/// One hit event registration a script made (`RegisterForHitEvent`).
#[derive(Debug, Clone, PartialEq)]
pub struct HitRegistration {
    /// The instance that registered (it receives `OnHit`).
    pub inst: InstId,
    /// The object it registered for: a reference, or an alias object whose
    /// holder is the hit reference.
    pub target: ObjId,
    /// The aggressor filter: `None` for none given; the object otherwise.
    pub aggressor: Option<ObjId>,
    /// Another filter is set (a source, a projectile, or a power, sneak,
    /// bash or block filter other than -1), or a filter is undetermined: a
    /// caller's hit does not settle it, so the registration is not
    /// delivered (counted).
    pub filtered: bool,
    /// Made by the init reading (`rules::HIT_INIT_REGISTRATION`), not by a
    /// call the run ran.
    pub from_init: bool,
}

/// The run's fight state (kept in [`World`]).
#[derive(Debug, Default)]
pub struct Fights {
    /// Hit registrations kept and the caller's hits delivered
    /// (`rules::HIT_EVENTS`, `quest_run --hit-events`).
    pub hits: bool,
    /// The init reading (`rules::HIT_INIT_REGISTRATION`,
    /// `quest_run --hit-init-registrations`).
    pub init_registrations: bool,
    /// The caller's combat state changes delivered
    /// (`rules::COMBAT_STATE_EVENTS`, `quest_run --combat-state-events`).
    pub combat_events: bool,
    /// The caller's deaths delivered (`rules::DEATH_EVENTS`,
    /// `quest_run --death-events`).
    pub death_events: bool,
    /// `IsDead` answered on the actors the run killed (`rules::DEAD_STATE`,
    /// `quest_run --dead-state`).
    pub dead_state: bool,
    /// `IsInCombat` and `GetCombatState` answered from the run's states
    /// (`rules::COMBAT_STATE_NATIVES`, `quest_run --combat-state-natives`).
    pub combat_natives: bool,
    /// Hit registrations, in the order made.
    pub registrations: Vec<HitRegistration>,
    /// Instances the init reading looked at (once each).
    pub init_read: BTreeSet<InstId>,
    /// The last combat state the caller assumed for an actor: (target,
    /// state).
    pub states: BTreeMap<ObjId, (ObjId, i32)>,
    /// Actors whose death the run raised.
    pub dead: BTreeSet<ObjId>,
    /// (real-time second, what happened), in order.
    pub log: Vec<(f64, String)>,
    pub counts: BTreeMap<String, u64>,
    /// Rules relied on inside a native call, taken by the host after it.
    pub used: Vec<&'static str>,
}
impl Fights {
    pub fn count(&mut self, k: impl Into<String>) {
        *self.counts.entry(k.into()).or_default() += 1;
    }
    fn using(&mut self, r: &'static str) {
        if !self.used.contains(&r) {
            self.used.push(r);
        }
    }
    /// Whether any switch is on.
    pub fn any(&self) -> bool {
        self.hits
            || self.init_registrations
            || self.combat_events
            || self.death_events
            || self.dead_state
            || self.combat_natives
    }
    /// Whether the actor is in combat in the run: its last assumed state
    /// is not 0 and the run has not killed it.
    fn fighting(&self, o: ObjId) -> Option<i32> {
        if self.dead.contains(&o) {
            return Some(0);
        }
        self.states.get(&o).map(|(_, s)| *s)
    }
}

/// The natives the fight model keeps or answers.
pub const NATIVES: [&str; 6] = [
    "scriptobject.registerforhitevent",
    "scriptobject.unregisterforhitevent",
    "scriptobject.unregisterforallhitevents",
    "actor.isdead",
    "actor.isincombat",
    "actor.getcombatstate",
];

/// The event function names.
pub const ON_HIT: &str = "onhit";
pub const ON_COMBAT_STATE: &str = "oncombatstatechanged";
pub const ON_DYING: &str = "ondying";
pub const ON_DEATH: &str = "ondeath";

/// The init handlers the init reading reads.
pub const INIT_HANDLERS: [&str; 2] = ["onaliasinit", "oninit"];

fn object(v: Option<&Val>) -> Option<Option<ObjId>> {
    match v {
        Some(Val::Object(o)) => Some(Some(*o)),
        Some(Val::None) | None => Some(None),
        _ => None,
    }
}

/// The fight natives, each under its switch; `None` when the switch is off
/// or the call is not one the model answers (recorded as before).
pub fn native(c: &NativeCall, w: &mut World) -> Option<Reply> {
    let full = c.full_name();
    if !NATIVES.contains(&full.as_str()) || !w.fights.any() {
        return None;
    }
    match full.as_str() {
        "scriptobject.registerforhitevent" if w.fights.hits => {
            w.fights.using(rules::HIT_EVENTS);
            let Some(inst) = c.instance else {
                w.fights
                    .count("hit registrations\tnot kept: the calling instance is undetermined");
                return Some(Reply::Value(Val::None));
            };
            let Some(Some(target)) = object(c.args.first()) else {
                w.fights
                    .count("hit registrations\tnot kept: the target is none or undetermined");
                return Some(Reply::Value(Val::None));
            };
            let aggressor = object(c.args.get(1));
            let others = c
                .args
                .iter()
                .skip(2)
                .take(2)
                .all(|v| matches!(v, Val::None))
                && c.args
                    .iter()
                    .skip(4)
                    .take(4)
                    .all(|v| matches!(v, Val::Int(-1)));
            let filtered = aggressor.is_none() || !others;
            let r = HitRegistration {
                inst,
                target,
                aggressor: aggressor.flatten(),
                filtered,
                from_init: false,
            };
            if !w.fights.registrations.contains(&r) {
                w.fights.registrations.push(r);
            }
            w.fights.count(if filtered {
                "hit registrations\tkept, with a filter a caller's hit does not settle"
            } else {
                "hit registrations\tkept"
            });
            Some(Reply::Value(Val::None))
        }
        "scriptobject.unregisterforhitevent" | "scriptobject.unregisterforallhitevents"
            if w.fights.hits =>
        {
            w.fights.using(rules::HIT_EVENTS);
            let Some(inst) = c.instance else {
                w.fights
                    .count("hit registrations\tnot removed: the calling instance is undetermined");
                return Some(Reply::Value(Val::None));
            };
            let target = match object(c.args.first()) {
                Some(t) => t,
                None => {
                    w.fights
                        .count("hit registrations\tnot removed: the target is undetermined");
                    return Some(Reply::Value(Val::None));
                }
            };
            let before = w.fights.registrations.len();
            w.fights
                .registrations
                .retain(|r| !(r.inst == inst && target.is_none_or(|t| r.target == t)));
            let n = before - w.fights.registrations.len();
            *w.fights
                .counts
                .entry("hit registrations\tremoved".into())
                .or_default() += n as u64;
            Some(Reply::Value(Val::None))
        }
        "actor.isdead" if w.fights.dead_state => {
            let Some(Val::Object(o)) = c.receiver.clone() else {
                return None;
            };
            if !w.fights.dead.contains(&o) {
                if w.quests.created_refs && crate::quest::is_created(w, o) {
                    w.fights.using(rules::ALIAS_CREATED_REFERENCE);
                    w.fights.count(
                        "IsDead answered: false (a reference the run made for a created alias, not killed)",
                    );
                    return Some(Reply::Value(Val::Bool(false)));
                }
                w.fights
                    .count("IsDead not answered: the run raised no death of the actor");
                return None;
            }
            w.fights.using(rules::DEAD_STATE);
            w.fights
                .count("IsDead answered: true (the run killed the actor)");
            Some(Reply::Value(Val::Bool(true)))
        }
        "actor.isincombat" if w.fights.combat_natives => {
            let Some(Val::Object(o)) = c.receiver.clone() else {
                return None;
            };
            let v = if Some(o) == w.player {
                let player = o;
                let f = &w.fights;
                f.states
                    .iter()
                    .any(|(a, (t, s))| *t == player && *s != 0 && !f.dead.contains(a))
            } else {
                match w.fights.fighting(o) {
                    Some(s) => s != 0,
                    None => {
                        w.fights.count(
                            "IsInCombat not answered: the run gave the actor no combat state",
                        );
                        return None;
                    }
                }
            };
            w.fights.using(rules::COMBAT_STATE_NATIVES);
            w.fights.count(format!(
                "IsInCombat answered from the run's combat states on {}: {v}",
                if Some(o) == w.player {
                    "the player"
                } else {
                    "an actor"
                }
            ));
            Some(Reply::Value(Val::Bool(v)))
        }
        "actor.getcombatstate" if w.fights.combat_natives => {
            let Some(Val::Object(o)) = c.receiver.clone() else {
                return None;
            };
            let Some(s) = w.fights.fighting(o) else {
                w.fights
                    .count("GetCombatState not answered: the run gave the actor no combat state");
                return None;
            };
            w.fights.using(rules::COMBAT_STATE_NATIVES);
            w.fights
                .count("GetCombatState answered from the run's combat states");
            Some(Reply::Value(Val::Int(s)))
        }
        _ => None,
    }
}

/// One fight event raised: what happened and the handler runs.
#[derive(Debug)]
pub struct Raised {
    pub t: f64,
    pub what: String,
    pub runs: Vec<Delivery>,
}

fn now(vm: &Vm<'_>) -> f64 {
    vm.world.now[crate::world::Clock::Real as usize]
}

/// Whether an init handler of the instance (`OnAliasInit`, `OnInit`) calls
/// `RegisterForHitEvent` on itself with its own object as the target: the
/// first argument is `self` or a local the handler cast from `self`.
fn registers_on_init(vm: &mut Vm<'_>, inst: InstId) -> bool {
    for h in INIT_HANDLERS {
        let Ok(Some(f)) = vm.find_handler(inst, h) else {
            continue;
        };
        let mut from_self: BTreeSet<&str> = BTreeSet::from(["self"]);
        for i in &f.code {
            if i.op == 14 {
                if let (Some(Lit::Ident(d)), Some(Lit::Ident(s))) = (i.args.first(), i.args.get(1))
                {
                    if from_self.contains(s.as_str()) {
                        from_self.insert(d.as_str());
                    }
                }
            }
            if i.op == 23
                && matches!(i.args.first(), Some(Lit::Ident(n)) if n == "registerforhitevent")
                && matches!(i.args.get(1), Some(Lit::Ident(r)) if r == "self")
                && matches!(i.extra.first(), Some(Lit::Ident(a)) if from_self.contains(a.as_str()))
            {
                return true;
            }
        }
    }
    false
}

/// The objects that stand for a reference's receivers: the reference and
/// the alias objects of the aliases holding it.
fn receiver_objects(
    vm: &mut Vm<'_>,
    store: &Store,
    target: ObjId,
    b: &Budget,
) -> Result<Vec<ObjId>, Box<dyn std::error::Error>> {
    let mut out = vec![target];
    let key = vm.world.objects[target]
        .key
        .as_deref()
        .and_then(key_record)
        .ok_or("the hit reference is not a record")?;
    let (aliases, open) = player::holders(vm, store, &key);
    if !aliases.is_empty() || open > 0 {
        vm.rules.insert(rules::EVENT_ALIASES);
    }
    for (q, id) in aliases {
        let qo = crate::data::record_object(&mut vm.world, store, &q, "quest", b)?;
        let label = format!("alias {id} of {}", vm.world.objects[qo].label);
        let akey = format!("{}#alias:{id}", crate::bind::record_key(&q));
        out.push(vm.world.keyed_object(&label, &akey, "referencealias"));
    }
    Ok(out)
}

/// The player (or `aggressor`) hits the target (`rules::HIT_EVENTS`):
/// `OnHit(target, aggressor, none, none, false, false, false, false, "")`
/// to each instance registered for the target or for an alias holding it,
/// in registration order, each registration ended by its delivery. Under
/// `rules::HIT_INIT_REGISTRATION` the receivers' instances whose init
/// handler registers on their own object count as registered first.
pub fn hit(
    vm: &mut Vm<'_>,
    store: &Store,
    target: ObjId,
    aggressor: ObjId,
    b: &Budget,
) -> Result<Raised, Box<dyn std::error::Error>> {
    vm.rules.insert(rules::HIT_EVENTS);
    let objects = receiver_objects(vm, store, target, b)?;
    if vm.world.fights.init_registrations {
        for o in &objects {
            vm.attach_scripts(*o)?;
            for inst in vm.world.objects[*o].instances.clone() {
                if !vm.world.fights.init_read.insert(inst) {
                    continue;
                }
                if !registers_on_init(vm, inst) {
                    continue;
                }
                vm.rules.insert(rules::HIT_INIT_REGISTRATION);
                let r = HitRegistration {
                    inst,
                    target: *o,
                    aggressor: None,
                    filtered: false,
                    from_init: true,
                };
                if !vm.world.fights.registrations.contains(&r) {
                    vm.world.fights.registrations.push(r);
                    vm.world.fights.count(
                        "hit registrations\tread from an init handler the run never delivered",
                    );
                }
            }
        }
    }
    let t = now(vm);
    let what = format!(
        "{} hits {}",
        vm.world.objects[aggressor].label, vm.world.objects[target].label
    );
    vm.world.fights.log.push((t, what.clone()));
    vm.world.fights.count("hits");
    let mut taken = Vec::new();
    let mut kept = Vec::new();
    for r in std::mem::take(&mut vm.world.fights.registrations) {
        if !objects.contains(&r.target) {
            kept.push(r);
        } else if r.filtered || r.aggressor.is_some_and(|a| a != aggressor) {
            vm.world.fights.count(
                "hits\ta registration not delivered: its filters do not match or are not settled",
            );
            kept.push(r);
        } else {
            taken.push(r);
        }
    }
    vm.world.fights.registrations = kept;
    if taken.is_empty() {
        vm.world.fights.count("hits\tno registered instance");
    }
    let args = vec![
        Val::Object(target),
        Val::Object(aggressor),
        Val::None,
        Val::None,
        Val::Bool(false),
        Val::Bool(false),
        Val::Bool(false),
        Val::Bool(false),
        Val::Str(String::new()),
    ];
    let mut runs = Vec::new();
    for r in taken {
        let script = vm.world.instances[r.inst].script.clone();
        let outcome = match vm.find_handler(r.inst, ON_HIT) {
            Ok(Some(f)) if f.params.len() == args.len() => {
                vm.world.fights.count(if r.from_init {
                    "hits\tdelivered to an instance registered by the init reading"
                } else {
                    "hits\tdelivered to an instance registered in the run"
                });
                vm.deliver(r.inst, ON_HIT, args.clone())
            }
            Ok(Some(_)) => Outcome::Failed(VmError::BadArgument {
                site: None,
                detail: "event onhit takes parameters the event does not give".into(),
            }),
            Ok(None) => {
                vm.world
                    .fights
                    .count("hits\ta registered instance has no OnHit with code");
                continue;
            }
            Err(e) => Outcome::Failed(e),
        };
        runs.push(Delivery {
            receiver: Receiver::HitRegistered,
            script,
            outcome,
        });
    }
    Ok(Raised { t, what, runs })
}

/// The actor's combat state changes toward a target
/// (`rules::COMBAT_STATE_EVENTS`): `OnCombatStateChanged(target, state)` on
/// the actor's own scripts, the aliases holding it and the instances
/// registered for its remote event. The state is kept for the natives
/// (`rules::COMBAT_STATE_NATIVES`).
pub fn combat_state(
    vm: &mut Vm<'_>,
    store: &Store,
    actor: ObjId,
    target: ObjId,
    state: i32,
    b: &Budget,
) -> Result<Raised, Box<dyn std::error::Error>> {
    vm.rules.insert(rules::COMBAT_STATE_EVENTS);
    vm.world.fights.states.insert(actor, (target, state));
    vm.world.fights.count("combat state changes");
    let t = now(vm);
    let what = format!(
        "{} changes its combat state to {state} toward {}",
        vm.world.objects[actor].label, vm.world.objects[target].label
    );
    vm.world.fights.log.push((t, what.clone()));
    let (runs, _) = player::deliver_event(
        vm,
        store,
        actor,
        ON_COMBAT_STATE,
        vec![Val::Object(target), Val::Int(state)],
        b,
    )?;
    Ok(Raised { t, what, runs })
}

/// The actor dies with a killer (`rules::DEATH_EVENTS`): `OnDying(killer)`
/// then `OnDeath(killer)` on its receivers as [`combat_state`] names them.
/// The run has it dead from then on (`rules::DEAD_STATE`,
/// `rules::COMBAT_STATE_NATIVES`).
pub fn death(
    vm: &mut Vm<'_>,
    store: &Store,
    actor: ObjId,
    killer: ObjId,
    b: &Budget,
) -> Result<Raised, Box<dyn std::error::Error>> {
    vm.rules.insert(rules::DEATH_EVENTS);
    if !vm.world.fights.dead.insert(actor) {
        vm.world
            .fights
            .count("deaths\tthe run had the actor dead already");
    }
    vm.world.fights.count("deaths");
    let t = now(vm);
    let what = format!(
        "{} dies, killed by {}",
        vm.world.objects[actor].label, vm.world.objects[killer].label
    );
    vm.world.fights.log.push((t, what.clone()));
    let (mut runs, _) =
        player::deliver_event(vm, store, actor, ON_DYING, vec![Val::Object(killer)], b)?;
    let (more, _) =
        player::deliver_event(vm, store, actor, ON_DEATH, vec![Val::Object(killer)], b)?;
    runs.extend(more);
    Ok(Raised { t, what, runs })
}
