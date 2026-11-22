//! Events scripts send each other (`spec/vm.md`, Events): remote events
//! registered on another object and custom events, kept in the run's world
//! and delivered only where the interpreter itself raises the event. Every
//! behaviour is a pending rule (`rules::REMOTE_EVENTS`,
//! `rules::CUSTOM_EVENTS`, `rules::CUSTOM_EVENT_ORDER`); nothing is
//! measured in the running game.
//!
//! The handler names are read from the compiled scripts: a function of a
//! registering script named `::remote_<type>_<event>` for a remote event
//! (its first parameter the sender, typed by the engine type that declares
//! the event) and `::remote_<event>` for a custom event whose name the
//! sending and registering calls store as `<script>_<event>` (parameters:
//! the sender and an array of values).
use crate::{
    rules,
    world::{InstId, ObjId, Val, World},
    Outcome, Vm, VmError,
};
use std::collections::{BTreeMap, VecDeque};

/// One registration: the instance that registered, the object it listens
/// to, the event name (lower case, as stored in the registering call), and
/// whether it is a custom event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub instance: InstId,
    pub sender: ObjId,
    pub event: String,
    pub custom: bool,
}

/// When a custom event is delivered (`vm_custom_event_order_v1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CustomOrder {
    /// The candidate: after the outermost delivery or job running when it
    /// was sent ends, in sending order.
    #[default]
    Queued,
    /// The alternative: at once, inside the `SendCustomEvent` call.
    Nested,
}

/// The registrations of a run and how events are delivered.
#[derive(Debug, Default)]
pub struct Events {
    pub registrations: Vec<Registration>,
    /// Deliver remote events the interpreter raises (`vm_remote_events_v1`).
    pub remote: bool,
    /// Deliver custom events (`vm_custom_events_v1`), and when.
    pub custom: Option<CustomOrder>,
    /// Custom events sent under [`CustomOrder::Queued`], waiting for the
    /// delivery that sent them to end.
    pub later: VecDeque<crate::quest::Job>,
    /// Counts by text (registrations, deliveries, handlers).
    pub counts: BTreeMap<String, u64>,
}
impl Events {
    pub fn count(&mut self, k: impl Into<String>) {
        *self.counts.entry(k.into()).or_default() += 1;
    }
    /// Registers (once) an instance for an event of a sender.
    pub fn register(&mut self, r: Registration) {
        if !self.registrations.contains(&r) {
            self.registrations.push(r);
        }
    }
    /// Removes registrations of an instance: of one sender and event, or
    /// all of one kind when `sender` is `None`.
    pub fn unregister(
        &mut self,
        instance: InstId,
        sender: Option<ObjId>,
        event: Option<&str>,
        custom: bool,
    ) {
        self.registrations.retain(|r| {
            !(r.instance == instance
                && r.custom == custom
                && sender.is_none_or(|s| s == r.sender)
                && event.is_none_or(|e| e == r.event))
        });
    }
    /// The registrations for an event of a sender, in registration order.
    pub fn listeners(&self, sender: ObjId, event: &str, custom: bool) -> Vec<InstId> {
        self.registrations
            .iter()
            .filter(|r| r.sender == sender && r.custom == custom && r.event == event)
            .map(|r| r.instance)
            .collect()
    }
}

/// The lower-case names of the types an object is known to be, with every
/// parent: its known types, its record type's engine type and its attached
/// scripts.
fn sender_types(vm: &mut Vm<'_>, o: ObjId) -> Vec<String> {
    let mut start: Vec<String> = vm.world.objects[o].types.clone();
    if let Some(t) = vm.world.objects[o].signature_type.clone() {
        start.push(t);
    }
    for i in vm.world.objects[o].instances.clone() {
        start.push(vm.world.instances[i].script.clone());
    }
    let mut out: Vec<String> = Vec::new();
    for t in start {
        let chain = match vm.chain(&t) {
            Ok(c) => c.iter().map(|s| s.name.clone()).collect(),
            Err(_) => vec![t],
        };
        for n in chain {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

/// Delivers the remote event `event` (lower case, without `on` stripped:
/// for example `onstageset`) raised on `sender` to every instance
/// registered for it (`vm_remote_events_v1`), in registration order, after the
/// sender's own scripts received it. The handler is the instance's
/// function `::remote_<type>_<event>` for a type the sender is; `args`
/// gives the arguments after the sender from the handler's parameters
/// (`None`: the handler takes parameters the event does not give).
#[allow(clippy::type_complexity)]
pub fn deliver_remote(
    vm: &mut Vm<'_>,
    sender: ObjId,
    event: &str,
    args: &mut dyn FnMut(&mut Vm<'_>, &[(String, String)]) -> Option<Vec<Val>>,
    nested: bool,
) -> Vec<(String, Outcome)> {
    let mut out = Vec::new();
    if !vm.world.events.remote {
        return out;
    }
    let listeners = vm.world.events.listeners(sender, event, false);
    if listeners.is_empty() {
        return out;
    }
    vm.rules.insert(rules::REMOTE_EVENTS);
    let types = sender_types(vm, sender);
    for inst in listeners {
        let script = vm.world.instances[inst].script.clone();
        let mut found = Vec::new();
        for t in &types {
            let name = format!("::remote_{t}_{event}");
            match vm.find_handler(inst, &name) {
                Ok(Some(f)) => found.push((name, f)),
                Ok(None) => {}
                Err(e) => {
                    out.push((script.clone(), Outcome::Failed(e)));
                    continue;
                }
            }
        }
        vm.world.events.count(format!(
            "remote deliveries\t{event}\t{} handler(s)",
            found.len()
        ));
        let (name, f) = match found.len() {
            0 => continue,
            1 => found.remove(0),
            _ => {
                out.push((
                    script,
                    Outcome::Failed(VmError::BadArgument {
                        site: None,
                        detail: format!("two remote handlers for {event}"),
                    }),
                ));
                continue;
            }
        };
        let Some(rest) = f.params.get(1..).and_then(|p| args(vm, p)) else {
            out.push((
                script,
                Outcome::Failed(VmError::BadArgument {
                    site: None,
                    detail: format!("{name} takes parameters the event does not give"),
                }),
            ));
            continue;
        };
        let mut a = vec![Val::Object(sender)];
        a.extend(rest);
        let o = if nested {
            vm.deliver_nested(inst, &name, a)
        } else {
            vm.deliver(inst, &name, a)
        };
        out.push((script, o));
    }
    out
}

/// Delivers a custom event sent by `sender` (`vm_custom_events_v1`): to every
/// instance registered for (sender, event), the function
/// `::remote_<event>` with the sender and the arguments array.
pub fn deliver_custom(
    vm: &mut Vm<'_>,
    sender: ObjId,
    event: &str,
    args: Val,
    nested: bool,
) -> Vec<(String, Outcome)> {
    let mut out = Vec::new();
    vm.rules.insert(rules::CUSTOM_EVENTS);
    let name = format!("::remote_{event}");
    for inst in vm.world.events.listeners(sender, event, true) {
        let script = vm.world.instances[inst].script.clone();
        let f = match vm.find_handler(inst, &name) {
            Ok(Some(f)) => f,
            Ok(None) => {
                vm.world.events.count("custom deliveries\tno handler");
                continue;
            }
            Err(e) => {
                out.push((script, Outcome::Failed(e)));
                continue;
            }
        };
        if f.params.len() != 2 {
            out.push((
                script,
                Outcome::Failed(VmError::BadArgument {
                    site: None,
                    detail: format!("{name} does not take a sender and an array"),
                }),
            ));
            continue;
        }
        vm.world.events.count("custom deliveries\ta handler");
        let a = vec![Val::Object(sender), args.clone()];
        let o = if nested {
            vm.deliver_nested(inst, &name, a)
        } else {
            vm.deliver(inst, &name, a)
        };
        out.push((script, o));
    }
    out
}

/// What the event natives do (`vm_remote_events_v1`, `vm_custom_events_v1`):
/// `None` for a native that is not one of them.
pub fn native(c: &crate::NativeCall, w: &mut World, depth: usize) -> Option<crate::Reply> {
    use crate::Reply;
    let full = c.full_name();
    let obj = |v: Option<&Val>| match v {
        Some(Val::Object(o)) => Some(*o),
        _ => None,
    };
    let text = |v: Option<&Val>| match v {
        Some(Val::Str(s)) => Some(s.to_ascii_lowercase()),
        _ => None,
    };
    let me = c.instance;
    match full.as_str() {
        "scriptobject.registerforremoteevent" | "scriptobject.registerforcustomevent" => {
            let custom = full.ends_with("customevent");
            let kind = if custom { "custom" } else { "remote" };
            match (me, obj(c.args.first()), text(c.args.get(1))) {
                (Some(i), Some(s), Some(e)) => {
                    w.events.count(format!("registrations\t{kind}\tmade"));
                    if !custom {
                        w.events
                            .count(format!("registrations\tremote\tby event\t{e}"));
                    }
                    w.events.register(Registration {
                        instance: i,
                        sender: s,
                        event: e,
                        custom,
                    });
                    Some(if custom {
                        Reply::Value(Val::None)
                    } else {
                        Reply::Value(Val::Bool(true))
                    })
                }
                _ => {
                    w.events
                        .count(format!("registrations\t{kind}\tnot made: an argument or the instance is undetermined"));
                    Some(if custom {
                        Reply::Value(Val::None)
                    } else {
                        Reply::Undetermined
                    })
                }
            }
        }
        "scriptobject.unregisterforremoteevent" | "scriptobject.unregisterforcustomevent" => {
            let custom = full.ends_with("customevent");
            if let Some(i) = me {
                match (obj(c.args.first()), text(c.args.get(1))) {
                    (Some(s), Some(e)) => w.events.unregister(i, Some(s), Some(&e), custom),
                    // Which registration goes is not settled: all of the
                    // instance's of that kind are taken to be open, so drop
                    // them (no delivery is better than a wrong one).
                    _ => w.events.unregister(i, None, None, custom),
                }
            }
            Some(Reply::Value(Val::None))
        }
        "scriptobject.unregisterforallremoteevents"
        | "scriptobject.unregisterforallcustomevents" => {
            if let Some(i) = me {
                w.events
                    .unregister(i, None, None, full.ends_with("customevents"));
            }
            Some(Reply::Value(Val::None))
        }
        "scriptobject.sendcustomevent" => {
            let Some(order) = w.events.custom else {
                w.events
                    .count("custom events sent\tnot delivered (switched off)");
                return Some(Reply::Value(Val::None));
            };
            match (c.receiver.as_ref(), text(c.args.first())) {
                (Some(Val::Object(s)), Some(e)) => {
                    w.events.count("custom events sent\tqueued for delivery");
                    let job = crate::quest::Job::Custom {
                        sender: *s,
                        event: e,
                        args: c.args.get(1).cloned().unwrap_or(Val::None),
                        depth,
                    };
                    match order {
                        CustomOrder::Queued => w.events.later.push_back(job),
                        CustomOrder::Nested => w.quests.queue.push_back(job),
                    }
                }
                _ => w.events.count(
                    "custom events sent\tnot delivered: the sender or the name is undetermined",
                ),
            }
            Some(Reply::Value(Val::None))
        }
        _ => None,
    }
}

/// The natives [`native`] answers.
pub const NATIVES: [&str; 7] = [
    "scriptobject.registerforremoteevent",
    "scriptobject.registerforcustomevent",
    "scriptobject.unregisterforremoteevent",
    "scriptobject.unregisterforcustomevent",
    "scriptobject.unregisterforallremoteevents",
    "scriptobject.unregisterforallcustomevents",
    "scriptobject.sendcustomevent",
];
