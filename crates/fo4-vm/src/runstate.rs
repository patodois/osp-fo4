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
//! reference stores a lock record), and `rules::GLOBAL_VALUE_UNDETERMINED`
//! (a `SetValue` whose value is undetermined leaves the global undetermined
//! for `GetValue` until a known value is set; without it `GetValue`
//! answers the stored `FLTV` again). The caller's `--assume-item-count`
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
pub const NATIVES: [&str; 13] = [
    "globalvariable.setvalue",
    "globalvariable.getvalue",
    "objectreference.lock",
    "objectreference.islocked",
    "objectreference.getitemcount",
    "objectreference.getvalue",
    "actor.hasperk",
    "actor.addperk",
    "actor.removeperk",
    "actor.isinfaction",
    "actor.setfactionrank",
    "actor.removefromfaction",
    "actor.removefromallfactions",
];

/// The run's held state (kept in [`World`]).
#[derive(Debug, Default)]
pub struct RunState {
    /// `rules::GLOBAL_RUNTIME_VALUE` (`quest_run --global-values`).
    pub globals: bool,
    /// `rules::GLOBAL_VALUE_UNDETERMINED` (`quest_run
    /// --global-value-undetermined`, with `globals`).
    pub undetermined_values: bool,
    /// Globals a `SetValue` left undetermined under that reading, by
    /// record key.
    pub open_values: std::collections::BTreeSet<String>,
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
    /// `rules::LOCK_STATE_EVENT_SCRIPT` (`quest_run --lock-events-script`,
    /// with `locks`): a script's `Lock` that changes the held lock raises
    /// `OnLockStateChanged` after the call.
    pub lock_events_script: bool,
    /// References whose held lock a script changed, waiting for their
    /// event (`deliver_lock_changes`).
    pub lock_changed: Vec<ObjId>,
    /// `rules::TERMINAL_FRAGMENT` (`quest_run --terminal-fragments`).
    pub terminals: bool,
    /// `rules::PERK_BASE_LIST` (`quest_run --perks-base`).
    pub perks: bool,
    /// Perks the run's `AddPerk` (true) and `RemovePerk` (false) gave the
    /// player, by the perk's record key.
    pub player_perks: BTreeMap<String, bool>,
    /// `rules::FACTION_BASE_LIST` (`quest_run --factions-base`).
    pub factions: bool,
    /// Faction changes the run's `SetFactionRank` (true),
    /// `RemoveFromFaction` (false) made: (actor key or `player`, faction
    /// key) to membership.
    pub faction_changes: BTreeMap<(String, String), bool>,
    /// Actors the run's `RemoveFromAllFactions` emptied (keys or `player`).
    pub factions_emptied: std::collections::BTreeSet<String>,
    /// The caller's actor values: (holder key or `player`, actor value
    /// key) to the value (`quest_run --assume-actor-value`).
    pub actor_values: BTreeMap<(String, String), f32>,
    /// The inventory readings and the filters they keep
    /// (`crate::inventory`).
    pub inventory: crate::inventory::Inventory,
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
    pub(crate) fn using(&mut self, r: &'static str) {
        if !self.used.contains(&r) {
            self.used.push(r);
        }
    }
    /// Whether anything here is on.
    pub fn any(&self) -> bool {
        self.globals
            || self.locks
            || !self.item_counts.is_empty()
            || self.perks
            || self.factions
            || !self.actor_values.is_empty()
            || self.inventory.any()
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
    if crate::inventory::NATIVES.contains(&full.as_str()) {
        return crate::inventory::native(c, w);
    }
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
                    // An undetermined value: the run's value is dropped
                    // (and, under the undetermined reading, the global
                    // answers undetermined until a known value is set).
                    w.runstate.values.remove(&k);
                    w.runstate
                        .count("global values\tset to an undetermined value");
                    if w.runstate.undetermined_values {
                        w.runstate.using(rules::GLOBAL_VALUE_UNDETERMINED);
                        w.runstate.open_values.insert(k.clone());
                        w.runstate
                            .log
                            .push((now, format!("global {k} set to an undetermined value")));
                    }
                    return None;
                }
            };
            w.runstate.open_values.remove(&k);
            w.runstate.values.insert(k.clone(), v);
            w.runstate.log.push((now, format!("global {k} set to {v}")));
            w.runstate.count("global values\tset");
            Some(Reply::Value(Val::None))
        }
        "globalvariable.getvalue" if w.runstate.globals => {
            let k = receiver_key(c, w)?;
            if w.runstate.undetermined_values && w.runstate.open_values.contains(&k) {
                w.runstate.using(rules::GLOBAL_RUNTIME_VALUE);
                w.runstate.using(rules::GLOBAL_VALUE_UNDETERMINED);
                w.runstate
                    .count("global values\tanswered undetermined (set to an undetermined value)");
                return Some(Reply::Undetermined);
            }
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
            if w.runstate.lock_events_script {
                let before = match w.runstate.locked.get(&k) {
                    Some(v) => Some(*v),
                    None => stores_lock(store, max, &k),
                };
                if before != Some(*lock) {
                    if let Some(Val::Object(o)) = &c.receiver {
                        w.runstate.using(rules::LOCK_STATE_EVENT_SCRIPT);
                        w.runstate.lock_changed.push(*o);
                        w.runstate
                            .count("locks\ta script's lock change waits for its event");
                    }
                }
            }
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
        "objectreference.getvalue" if !w.runstate.actor_values.is_empty() => {
            let holder = receiver_key(c, w)?;
            let av = arg_key(c.args.first(), w)?;
            let v = *w.runstate.actor_values.get(&(holder, av))?;
            w.runstate
                .count("actor values\tanswered from the caller's assumption");
            Some(Reply::Value(Val::Float(v)))
        }
        "actor.hasperk" if w.runstate.perks => {
            if receiver_key(c, w).as_deref() != Some("player") {
                return None;
            }
            let perk = arg_key(c.args.first(), w)?;
            if let Some(v) = w.runstate.player_perks.get(&perk) {
                let v = *v;
                w.runstate.using(rules::PERK_BASE_LIST);
                w.runstate
                    .count("perks\tanswered from the run's AddPerk or RemovePerk");
                return Some(Reply::Value(Val::Bool(v)));
            }
            let base = w.player_base_object.clone()?;
            let listed = base_perks(store, max, &base)?;
            w.runstate.using(rules::PERK_BASE_LIST);
            w.runstate
                .count("perks\tanswered from the player's base record");
            Some(Reply::Value(Val::Bool(listed.contains(&perk))))
        }
        "actor.addperk" | "actor.removeperk" if w.runstate.perks => {
            if receiver_key(c, w).as_deref() != Some("player") {
                return None;
            }
            let perk = arg_key(c.args.first(), w)?;
            let add = full == "actor.addperk";
            w.runstate.using(rules::PERK_BASE_LIST);
            w.runstate.player_perks.insert(perk.clone(), add);
            w.runstate.log.push((
                now,
                format!("{} {perk}", if add { "perk added" } else { "perk removed" }),
            ));
            w.runstate.count(format!(
                "perks\t{} on the player",
                if add { "added" } else { "removed" }
            ));
            Some(Reply::Value(Val::None))
        }
        "actor.isinfaction" if w.runstate.factions => {
            let actor = receiver_key(c, w)?;
            let faction = arg_key(c.args.first(), w)?;
            if let Some(v) = w
                .runstate
                .faction_changes
                .get(&(actor.clone(), faction.clone()))
            {
                let v = *v;
                w.runstate.using(rules::FACTION_BASE_LIST);
                w.runstate
                    .count("factions\tanswered from the run's faction changes");
                return Some(Reply::Value(Val::Bool(v)));
            }
            if w.runstate.factions_emptied.contains(&actor) {
                w.runstate.using(rules::FACTION_BASE_LIST);
                w.runstate
                    .count("factions\tanswered from the run's RemoveFromAllFactions");
                return Some(Reply::Value(Val::Bool(false)));
            }
            let base = if actor == "player" {
                w.player_base_object.clone()?
            } else {
                let k = key_record(&actor)?;
                crate::bind::base_of(store, &k, &Budget::new(max)).ok()??
            };
            let Some(listed) = base_factions(store, max, &base) else {
                w.runstate.count(
                    "factions\tnot answered: the base is a template's or its list is not read",
                );
                return None;
            };
            w.runstate.using(rules::FACTION_BASE_LIST);
            w.runstate
                .count("factions\tanswered from the actor's base record");
            Some(Reply::Value(Val::Bool(listed.contains(&faction))))
        }
        "actor.setfactionrank" | "actor.removefromfaction" if w.runstate.factions => {
            let actor = receiver_key(c, w)?;
            let faction = arg_key(c.args.first(), w)?;
            let member = match full.as_str() {
                "actor.removefromfaction" => false,
                _ => match c.args.get(1) {
                    Some(Val::Int(r)) => *r >= 0,
                    _ => return None,
                },
            };
            w.runstate.using(rules::FACTION_BASE_LIST);
            w.runstate
                .faction_changes
                .insert((actor.clone(), faction.clone()), member);
            w.runstate.log.push((
                now,
                format!(
                    "{actor} {} {faction}",
                    if member { "joins" } else { "leaves" }
                ),
            ));
            w.runstate.count(format!(
                "factions\t{} by the run",
                if member { "joined" } else { "left" }
            ));
            Some(Reply::Value(Val::None))
        }
        "actor.removefromallfactions" if w.runstate.factions => {
            let actor = receiver_key(c, w)?;
            w.runstate.using(rules::FACTION_BASE_LIST);
            w.runstate.faction_changes.retain(|(a, _), _| *a != actor);
            w.runstate.factions_emptied.insert(actor);
            w.runstate.count("factions\tall left by the run");
            Some(Reply::Value(Val::None))
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

/// The perks an actor base record lists (`PRKR`: a perk FormID, then a
/// rank byte), as record keys; `None` when the record cannot be read or a
/// `PRKR` is not 5 bytes.
fn base_perks(store: &Store, max: u64, base: &fo4_plugin::FormKey) -> Option<Vec<String>> {
    let (r, plugin) = store.raw_with_plugin(base, &Budget::new(max)).ok()??;
    let mut out = Vec::new();
    for s in r.subrecords.iter().filter(|s| s.signature == *b"PRKR") {
        let d = r.data(s);
        if d.len() != 5 {
            return None;
        }
        let id = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
        if let Some(k) = store
            .order()
            .remap(plugin, id, &r.source.file, s.offset)
            .ok()
            .flatten()
        {
            out.push(crate::bind::record_key(&k));
        }
    }
    Some(out)
}

/// The factions an actor base record lists with a rank of 0 or more
/// (`SNAM`: a faction FormID, then a signed rank byte), as record keys;
/// `None` when the record cannot be read, names a template (`TPLT`) or a
/// `SNAM` is not 5 bytes.
fn base_factions(store: &Store, max: u64, base: &fo4_plugin::FormKey) -> Option<Vec<String>> {
    let (r, plugin) = store.raw_with_plugin(base, &Budget::new(max)).ok()??;
    if r.signature != *b"NPC_" || r.subrecords.iter().any(|s| s.signature == *b"TPLT") {
        return None;
    }
    let mut out = Vec::new();
    for s in r.subrecords.iter().filter(|s| s.signature == *b"SNAM") {
        let d = r.data(s);
        if d.len() != 5 {
            return None;
        }
        if (d[4] as i8) < 0 {
            continue;
        }
        let id = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
        if let Some(k) = store
            .order()
            .remap(plugin, id, &r.source.file, s.offset)
            .ok()
            .flatten()
        {
            out.push(crate::bind::record_key(&k));
        }
    }
    Some(out)
}

/// Delivers `OnLockStateChanged()` for each reference whose held lock a
/// script's `Lock` changed since the last call
/// (`rules::LOCK_STATE_EVENT_SCRIPT`), in the order changed.
pub fn deliver_lock_changes(
    vm: &mut Vm<'_>,
    store: &Store,
    b: &Budget,
) -> Result<Vec<Raised>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    for o in std::mem::take(&mut vm.world.runstate.lock_changed) {
        let t = vm.world.now[crate::world::Clock::Real as usize];
        let what = format!("a script's lock change on {}", vm.world.objects[o].label);
        vm.rules.insert(rules::LOCK_STATE_EVENT_SCRIPT);
        vm.world.runstate.log.push((t, what.clone()));
        vm.world
            .runstate
            .count("locks\ta script's lock change raised its event");
        let runs = player::deliver_event(vm, store, o, ON_LOCK_STATE_CHANGED, Vec::new(), b)?.0;
        out.push(Raised { t, what, runs });
    }
    Ok(out)
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
