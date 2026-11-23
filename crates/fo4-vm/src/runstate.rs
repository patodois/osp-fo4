//! State the run keeps for records the data stores once (`spec/vm.md`, The
//! museum's order): the value of a global variable the run's scripts set,
//! the lock of a placed reference the run's scripts lock or unlock, and the
//! caller's item counts.
//!
//! Each behaviour is a pending rule behind its own switch, off by default:
//! `rules::GLOBAL_RUNTIME_VALUE` (`GlobalVariable.SetValue` changes the
//! value `GetValue` answers from then on; before it, the stored `FLTV` as
//! the data host reads it) and `rules::LOCK_STATE` (`ObjectReference.Lock`
//! changes the lock `IsLocked` answers; before it, whether the placed
//! reference stores a lock record). The caller's `--assume-item-count`
//! answers `GetItemCount` of one item on one holder; it is an assumption,
//! not a reading. Nothing is measured in the running game.
use crate::{
    data::key_record,
    fight::Raised,
    host::{NativeCall, Reply},
    player, rules,
    world::{ObjId, Val, World},
    Vm,
};
use fo4_core::Budget;
use fo4_plugin::typed::Store;
use std::collections::BTreeMap;

/// The natives this module answers when a switch turns it on.
pub const NATIVES: [&str; 5] = [
    "globalvariable.setvalue",
    "globalvariable.getvalue",
    "objectreference.lock",
    "objectreference.islocked",
    "objectreference.getitemcount",
];

/// The run's held state (kept in [`World`]).
#[derive(Debug, Default)]
pub struct RunState {
    /// `rules::GLOBAL_RUNTIME_VALUE` (`quest_run --global-values`).
    pub globals: bool,
    /// `rules::LOCK_STATE` (`quest_run --lock-state`).
    pub locks: bool,
    /// `rules::LOCK_STATE_EVENT` (`quest_run --lock-events`): the caller's
    /// unlock raises `OnLockStateChanged` on the reference's receivers.
    pub lock_events: bool,
    /// Values the run's `SetValue` gave, by the global's record key.
    pub values: BTreeMap<String, f32>,
    /// Locks the run's `Lock` gave, by the reference's record key.
    pub locked: BTreeMap<String, bool>,
    /// The caller's item counts: (holder key or `player`, item key) to the
    /// count (`quest_run --assume-item-count`).
    pub item_counts: BTreeMap<(String, String), i32>,
    /// (real-time second, what happened), in order.
    pub log: Vec<(f64, String)>,
    pub counts: BTreeMap<String, u64>,
    /// Rules relied on inside a native call, taken by the host after it.
    pub used: Vec<&'static str>,
}
impl RunState {
    pub fn count(&mut self, k: impl Into<String>) {
        *self.counts.entry(k.into()).or_default() += 1;
    }
    fn using(&mut self, r: &'static str) {
        if !self.used.contains(&r) {
            self.used.push(r);
        }
    }
    /// Whether anything here is on.
    pub fn any(&self) -> bool {
        self.globals || self.locks || !self.item_counts.is_empty()
    }
}

/// The receiver's record key, `player` for the player's handle.
fn receiver_key(c: &NativeCall, w: &World) -> Option<String> {
    let Some(Val::Object(o)) = &c.receiver else {
        return None;
    };
    if w.player == Some(*o) {
        return Some("player".into());
    }
    w.objects.get(*o)?.key.clone()
}

fn arg_key(v: Option<&Val>, w: &World) -> Option<String> {
    match v {
        Some(Val::Object(o)) => w.objects.get(*o)?.key.clone(),
        _ => None,
    }
}

/// Whether a placed reference's winning record stores a lock record
/// (`XLOC`); `None` when the record cannot be read.
fn stores_lock(store: &Store, max: u64, key: &str) -> Option<bool> {
    let k = key_record(key)?;
    let r = store.raw(&k, &Budget::new(max)).ok()??;
    Some(r.subrecords.iter().any(|s| s.signature == *b"XLOC"))
}

/// Answers the natives this module holds state for; `None` leaves the call
/// to the other hosts, as before.
pub fn native(store: &Store, max: u64, c: &NativeCall, w: &mut World) -> Option<Reply> {
    let full = c.full_name();
    if !NATIVES.contains(&full.as_str()) || !w.runstate.any() {
        return None;
    }
    let now = w.now[crate::world::Clock::Real as usize];
    match full.as_str() {
        "globalvariable.setvalue" if w.runstate.globals => {
            w.runstate.using(rules::GLOBAL_RUNTIME_VALUE);
            let Some(k) = receiver_key(c, w) else {
                w.runstate
                    .count("global values\tset on no record: recorded");
                return None;
            };
            let v = match c.args.first() {
                Some(Val::Float(f)) => *f,
                Some(Val::Int(i)) => *i as f32,
                _ => {
                    // An undetermined value: the global's value is open
                    // from now on.
                    w.runstate.values.remove(&k);
                    w.runstate
                        .count("global values\tset to an undetermined value");
                    return None;
                }
            };
            w.runstate.values.insert(k.clone(), v);
            w.runstate.log.push((now, format!("global {k} set to {v}")));
            w.runstate.count("global values\tset");
            Some(Reply::Value(Val::None))
        }
        "globalvariable.getvalue" if w.runstate.globals => {
            let k = receiver_key(c, w)?;
            let v = *w.runstate.values.get(&k)?;
            w.runstate.using(rules::GLOBAL_RUNTIME_VALUE);
            w.runstate.count("global values\tanswered from the run");
            Some(Reply::Value(Val::Float(v)))
        }
        "objectreference.lock" if w.runstate.locks => {
            w.runstate.using(rules::LOCK_STATE);
            let Some(k) = receiver_key(c, w) else {
                w.runstate.count("locks\tlock on no record: recorded");
                return None;
            };
            let Some(Val::Bool(lock)) = c.args.first() else {
                w.runstate.locked.remove(&k);
                w.runstate.count("locks\tan undetermined lock");
                return None;
            };
            w.runstate.locked.insert(k.clone(), *lock);
            w.runstate.log.push((
                now,
                format!("{} {k}", if *lock { "locked" } else { "unlocked" }),
            ));
            w.runstate.count(format!(
                "locks\t{}",
                if *lock { "locked" } else { "unlocked" }
            ));
            Some(Reply::Value(Val::None))
        }
        "objectreference.islocked" if w.runstate.locks => {
            let k = receiver_key(c, w)?;
            let v = match w.runstate.locked.get(&k) {
                Some(v) => *v,
                None => stores_lock(store, max, &k)?,
            };
            w.runstate.using(rules::LOCK_STATE);
            w.runstate.count(format!("locks\tanswered {v}"));
            Some(Reply::Value(Val::Bool(v)))
        }
        "objectreference.getitemcount" => {
            let holder = receiver_key(c, w)?;
            let item = arg_key(c.args.first(), w)?;
            let n = *w.runstate.item_counts.get(&(holder, item))?;
            w.runstate
                .count("item counts\tanswered from the caller's assumption");
            Some(Reply::Value(Val::Int(n)))
        }
        _ => None,
    }
}

/// The caller's unlock of a placed reference (`quest_run --assume-unlock`,
/// the player picking its lock): the run holds it unlocked from now on
/// (answered under `rules::LOCK_STATE`), and under
/// `rules::LOCK_STATE_EVENT` `OnLockStateChanged()` runs on the
/// reference's own scripts and the aliases holding it.
pub fn unlock(
    vm: &mut Vm<'_>,
    store: &Store,
    reference: ObjId,
    b: &Budget,
) -> Result<Raised, Box<dyn std::error::Error>> {
    let t = vm.world.now[crate::world::Clock::Real as usize];
    let what = format!("{} is unlocked", vm.world.objects[reference].label);
    if let Some(k) = vm.world.objects[reference].key.clone() {
        vm.world.runstate.locked.insert(k, false);
    }
    vm.world.runstate.log.push((t, what.clone()));
    vm.world.runstate.count("locks	unlocked by the caller");
    let runs = if vm.world.runstate.lock_events {
        vm.rules.insert(rules::LOCK_STATE_EVENT);
        player::deliver_event(vm, store, reference, ON_LOCK_STATE_CHANGED, Vec::new(), b)?.0
    } else {
        Vec::new()
    };
    Ok(Raised { t, what, runs })
}

/// The event a lock change raises.
pub const ON_LOCK_STATE_CHANGED: &str = "onlockstatechanged";
