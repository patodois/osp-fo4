//! Engine events a player action raises on a placed reference, delivered
//! only when a caller assumes the action (`quest_run --assume-activate`,
//! `--assume-trigger-enter`, `--assume-trigger-leave`, `--assume-open`):
//! the interpreter
//! has no player who moves or presses anything, so it raises none of them
//! by itself.
//!
//! Which scripts receive the event and with what arguments are pending
//! rules: the reference's own scripts (its base's VMAD, then its own) by
//! `vm_event_handlers_v1` (`vm_event_open_v1` for an opening) and
//! `vmad_reference_scripts_v1`, then the scripts
//! of every quest alias holding the reference (`vm_event_alias_scripts_v1`),
//! then the instances registered for the event as a remote event
//! (`vm_remote_events_v1`). Each handler is the function of the event's
//! name the compiled scripts declare, taking what that function's
//! parameters ask for (`bind::event_args`: none, or the player for one
//! object parameter); a handler taking anything else fails as a bad
//! argument. Nothing else an action does in the game (a door opening, an
//! item picked up, the default activation) is performed.
use crate::{
    bind,
    data::{key_record, record_object},
    events, quest, rules, ObjId, Outcome, Val, Vm, VmError,
};
use fo4_core::Budget;
use fo4_plugin::{typed::Store, FormKey};

/// A player action a caller can assume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    Activate,
    TriggerEnter,
    TriggerLeave,
    Open,
    /// A placed reference's 3D loads (`OnLoad`): not a player action; a
    /// caller's assumption (`--assume-load`) or a load the world model
    /// raises (`crate::load`). Not in [`Action::ALL`].
    Load,
}
impl Action {
    pub const ALL: [Action; 4] = [
        Action::Activate,
        Action::TriggerEnter,
        Action::TriggerLeave,
        Action::Open,
    ];
    /// The event function the action runs (`bind::EVENTS`).
    pub fn handler(self) -> &'static str {
        match self {
            Action::Activate => "onactivate",
            Action::TriggerEnter => "ontriggerenter",
            Action::TriggerLeave => "ontriggerleave",
            Action::Open => "onopen",
            Action::Load => "onload",
        }
    }
    /// The command-line switch that assumes it.
    pub fn switch(self) -> &'static str {
        match self {
            Action::Activate => "--assume-activate",
            Action::TriggerEnter => "--assume-trigger-enter",
            Action::TriggerLeave => "--assume-trigger-leave",
            Action::Open => "--assume-open",
            Action::Load => "--assume-load",
        }
    }
    /// What the action is, for counts.
    pub fn text(self) -> &'static str {
        match self {
            Action::Activate => "the player activates a placed reference",
            Action::TriggerEnter => "the player enters a placed trigger reference",
            Action::TriggerLeave => "the player leaves a placed trigger reference",
            Action::Open => "the player opens a placed reference (a door or a container)",
            Action::Load => "a placed reference's 3D loads",
        }
    }
    /// The action a command-line switch assumes: the player actions of
    /// [`Action::ALL`] and [`Action::Load`].
    pub fn from_switch(s: &str) -> Option<Action> {
        Action::ALL
            .into_iter()
            .chain([Action::Load])
            .find(|a| a.switch() == s)
    }
}

/// Who received an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Receiver {
    /// A script attached to the reference (base or reference VMAD).
    Reference,
    /// A script of a quest alias holding the reference: (quest handle,
    /// alias id).
    Alias(ObjId, i32),
    /// An instance registered for the event as a remote event.
    Registered,
}
impl Receiver {
    pub fn kind(&self) -> &'static str {
        match self {
            Receiver::Reference => "a script of the reference",
            Receiver::Alias(..) => "a script of a quest alias holding the reference",
            Receiver::Registered => "an instance registered for the remote event",
        }
    }
}

/// One handler run.
#[derive(Debug, Clone)]
pub struct Delivery {
    pub receiver: Receiver,
    pub script: String,
    pub outcome: Outcome,
}

/// The quest aliases holding a reference in the run's state
/// (`vm_event_alias_scripts_v1`): of every quest whose state the run holds
/// and that runs (its whole state settled), each reference alias whose
/// fill a run set is the reference, or, without such a fill, whose forced
/// reference (ALFR) it is (`quest_alias_forced_reference_v1`) or, with
/// `Quests::location_fills`, whose location special reference it is
/// (`quest_alias_location_ref_v1`); quests in
/// record key order, aliases by id. A quest whose running state is
/// undetermined is counted in the second value and left out.
pub fn holders(
    vm: &mut Vm<'_>,
    store: &Store,
    reference: &FormKey,
) -> (Vec<(FormKey, u32)>, usize) {
    let key = bind::record_key(reference);
    let mut out = Vec::new();
    let mut open = 0;
    let states: Vec<String> = vm.world.quests.states.keys().cloned().collect();
    for qk in states {
        let Some(q) = key_record(&qk) else { continue };
        let Some(info) = quest::info_by_key(store, store.limits.max_memory, &mut vm.world, &q)
        else {
            continue;
        };
        let s = &vm.world.quests.states[&qk];
        if s.open.is_some() || s.running.is_none() {
            open += 1;
            continue;
        }
        if s.running != Some(true) {
            continue;
        }
        for id in info.reference_aliases.keys() {
            let held = match i32::try_from(*id).ok().and_then(|i| s.fills.get(&i)) {
                Some(Val::Object(o)) => vm.world.objects[*o].key.as_deref() == Some(key.as_str()),
                Some(_) => false,
                None => {
                    vm.world.quests.fill(&info, *id).ok().flatten()
                        == Some(quest::Fill::Record(reference.clone()))
                }
            };
            if held {
                out.push((q.clone(), *id));
            }
        }
    }
    (out, open)
}

/// The arguments of an event handler: as [`bind::event_args`] gives them
/// (the player for one object parameter), or the actor that caused the
/// event in the player's place (`world_actor_trigger_v1`).
fn args_of(vm: &mut Vm<'_>, params: &[(String, String)], actor: Option<ObjId>) -> Option<Vec<Val>> {
    let player = bind::event_args(vm, params)?;
    Some(match (actor, player.as_slice()) {
        (Some(a), [_]) => vec![Val::Object(a)],
        _ => player,
    })
}

fn run_on(
    vm: &mut Vm<'_>,
    o: ObjId,
    handler: &str,
    receiver: Receiver,
    actor: Option<ObjId>,
    out: &mut Vec<Delivery>,
) {
    if let Err(e) = vm.attach_scripts(o) {
        out.push(Delivery {
            receiver,
            script: String::new(),
            outcome: Outcome::Failed(e),
        });
        return;
    }
    let insts = vm.world.objects[o].instances.clone();
    for inst in insts {
        let script = vm.world.instances[inst].script.clone();
        let func = match vm.find_handler(inst, handler) {
            Ok(Some(f)) => f,
            Ok(None) => continue,
            Err(e) => {
                out.push(Delivery {
                    receiver: receiver.clone(),
                    script,
                    outcome: Outcome::Failed(e),
                });
                continue;
            }
        };
        let outcome = match args_of(vm, &func.params, actor) {
            Some(args) => vm.deliver(inst, handler, args),
            None => Outcome::Failed(VmError::BadArgument {
                site: None,
                detail: format!("event {handler} takes parameters the event does not give"),
            }),
        };
        out.push(Delivery {
            receiver: receiver.clone(),
            script,
            outcome,
        });
    }
}

/// [`run_on`] with the handler's arguments given: a handler whose
/// parameters do not number as many fails as a bad argument.
fn run_on_with(
    vm: &mut Vm<'_>,
    o: ObjId,
    handler: &str,
    receiver: Receiver,
    args: &[Val],
    out: &mut Vec<Delivery>,
) {
    if let Err(e) = vm.attach_scripts(o) {
        out.push(Delivery {
            receiver,
            script: String::new(),
            outcome: Outcome::Failed(e),
        });
        return;
    }
    let insts = vm.world.objects[o].instances.clone();
    for inst in insts {
        let script = vm.world.instances[inst].script.clone();
        let func = match vm.find_handler(inst, handler) {
            Ok(Some(f)) => f,
            Ok(None) => continue,
            Err(e) => {
                out.push(Delivery {
                    receiver: receiver.clone(),
                    script,
                    outcome: Outcome::Failed(e),
                });
                continue;
            }
        };
        let outcome = if func.params.len() == args.len() {
            vm.deliver(inst, handler, args.to_vec())
        } else {
            Outcome::Failed(VmError::BadArgument {
                site: None,
                detail: format!("event {handler} takes parameters the event does not give"),
            })
        };
        out.push(Delivery {
            receiver: receiver.clone(),
            script,
            outcome,
        });
    }
}

/// The quest aliases holding the player in the run's state: of every
/// running quest whose state is settled, each reference alias whose fill a
/// run set is the player's handle, or, without such a fill, which holds the
/// player as a unique actor alias of the player's base
/// (`quest_alias_unique_actor_ref_v1`); as [`holders`] orders them.
pub fn player_holders(vm: &mut Vm<'_>, store: &Store) -> (Vec<(FormKey, u32)>, usize) {
    let player = vm.world.player;
    let mut out = Vec::new();
    let mut open = 0;
    let states: Vec<String> = vm.world.quests.states.keys().cloned().collect();
    for qk in states {
        let Some(q) = key_record(&qk) else { continue };
        let Some(info) = quest::info_by_key(store, store.limits.max_memory, &mut vm.world, &q)
        else {
            continue;
        };
        let s = &vm.world.quests.states[&qk];
        if s.open.is_some() || s.running.is_none() {
            open += 1;
            continue;
        }
        if s.running != Some(true) {
            continue;
        }
        for id in info.reference_aliases.keys() {
            let held = match i32::try_from(*id).ok().and_then(|i| s.fills.get(&i)) {
                Some(Val::Object(o)) => Some(*o) == player,
                Some(_) => false,
                None => {
                    vm.world.quests.fill(&info, *id).ok().flatten() == Some(quest::Fill::Player)
                }
            };
            if held {
                out.push((q.clone(), *id));
            }
        }
    }
    (out, open)
}

/// Delivers an engine event with the given arguments to an object the run
/// holds: a placed reference's own scripts and the aliases holding it, or,
/// for the player's handle, the aliases holding the player; then the
/// instances registered for it as a remote event (the arguments after the
/// sender). Used by furniture use (`crate::sit`).
pub fn deliver_event(
    vm: &mut Vm<'_>,
    store: &Store,
    target: ObjId,
    handler: &str,
    args: Vec<Val>,
    b: &Budget,
) -> Result<(Vec<Delivery>, usize), Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    let key = vm.world.objects[target].key.as_deref().and_then(key_record);
    let (aliases, open) = if Some(target) == vm.world.player {
        player_holders(vm, store)
    } else if let Some(k) = &key {
        run_on_with(vm, target, handler, Receiver::Reference, &args, &mut out);
        holders(vm, store, k)
    } else {
        return Err("the object is neither the player nor a record".into());
    };
    if !aliases.is_empty() || open > 0 {
        vm.rules.insert(rules::EVENT_ALIASES);
    }
    for (q, id) in aliases {
        let qo = record_object(&mut vm.world, store, &q, "quest", b)?;
        let label = format!("alias {id} of {}", vm.world.objects[qo].label);
        let akey = format!("{}#alias:{id}", bind::record_key(&q));
        let a = vm.world.keyed_object(&label, &akey, "referencealias");
        run_on_with(
            vm,
            a,
            handler,
            Receiver::Alias(qo, id as i32),
            &args,
            &mut out,
        );
    }
    let n = args.len();
    let mut rest = |_: &mut Vm<'_>, p: &[(String, String)]| (p.len() == n).then(|| args.clone());
    for (script, outcome) in events::deliver_remote(vm, target, handler, &mut rest, false) {
        out.push(Delivery {
            receiver: Receiver::Registered,
            script,
            outcome,
        });
    }
    Ok((out, open))
}

/// Delivers the event of `action` on a placed reference: to its own
/// scripts, then to the scripts of each alias holding it, then to the
/// instances registered for it. The jobs the handlers queue are left for
/// [`quest::run_jobs`]; with a [`quest::Runner`] the stages they set run
/// inside them. Returns the handler runs and the quests whose running
/// state left their aliases out.
pub fn deliver(
    vm: &mut Vm<'_>,
    store: &Store,
    reference: &FormKey,
    action: Action,
    b: &Budget,
) -> Result<(Vec<Delivery>, usize), Box<dyn std::error::Error>> {
    deliver_by(vm, store, reference, action, None, b)
}

/// [`deliver`] with an actor other than the player as the one who acts
/// (`world_actor_trigger_v1`): a handler's one object parameter is that
/// actor. `None` is the player, as [`deliver`].
pub fn deliver_by(
    vm: &mut Vm<'_>,
    store: &Store,
    reference: &FormKey,
    action: Action,
    actor: Option<ObjId>,
    b: &Budget,
) -> Result<(Vec<Delivery>, usize), Box<dyn std::error::Error>> {
    if !matches!(
        store.signature(reference).as_ref().map(|s| &s[..]),
        Some(b"REFR" | b"ACHR")
    ) {
        return Err("the FormID names no winning placed REFR or ACHR".into());
    }
    let handler = action.handler();
    vm.rules.insert(rules::EVENTS);
    if action == Action::Open {
        vm.rules.insert(rules::EVENT_OPEN);
    }
    let o = record_object(&mut vm.world, store, reference, "objectreference", b)?;
    let mut out = Vec::new();
    run_on(vm, o, handler, Receiver::Reference, actor, &mut out);
    let (aliases, open) = holders(vm, store, reference);
    if !aliases.is_empty() || open > 0 {
        vm.rules.insert(rules::EVENT_ALIASES);
    }
    for (q, id) in aliases {
        let qo = record_object(&mut vm.world, store, &q, "quest", b)?;
        let label = format!("alias {id} of {}", vm.world.objects[qo].label);
        let key = format!("{}#alias:{id}", bind::record_key(&q));
        let a = vm.world.keyed_object(&label, &key, "referencealias");
        run_on(
            vm,
            a,
            handler,
            Receiver::Alias(qo, id as i32),
            actor,
            &mut out,
        );
    }
    let mut args = |vm: &mut Vm<'_>, p: &[(String, String)]| args_of(vm, p, actor);
    for (script, outcome) in events::deliver_remote(vm, o, handler, &mut args, false) {
        out.push(Delivery {
            receiver: Receiver::Registered,
            script,
            outcome,
        });
    }
    Ok((out, open))
}

/// Parses a caller's FormID as the effect log writes record keys,
/// `plugin.esm:0001ABCD` (the object id in 8 hex digits, the load-order
/// byte ignored as the key ignores it).
pub fn parse_key(s: &str) -> Option<FormKey> {
    let (p, id) = s.rsplit_once(':')?;
    let id = u32::from_str_radix(id, 16).ok()?;
    key_record(&format!(
        "{}:{:08X}",
        p.to_ascii_lowercase(),
        id & 0x00ff_ffff
    ))
}
