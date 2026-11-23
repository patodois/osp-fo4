//! Events the town chain's step delivers from the caller's assumptions
//! (`spec/vm.md`, The town chain): an item that changes container
//! (`quest_run --assume-container-change`) and an actor that changes
//! location (`quest_run --assume-location-change`). The run keeps no
//! inventory and no location of its own: the caller says what moved, and
//! a reading behind its own switch, off by default, delivers the engine
//! event. Without the switch the caller's change is logged and counted and
//! nothing is raised. Nothing is measured in the running game.
//!
//! - `rules::CONTAINER_CHANGED_EVENT` (`--container-change-events`):
//!   `OnContainerChanged(new, old)` reaches the moved reference's own
//!   scripts, the aliases holding it and the instances registered for it.
//! - `rules::LOCATION_CHANGE_EVENT` (`--location-change-events`):
//!   `OnLocationChange(old, new)` reaches the actor's own scripts (none for
//!   the player), the aliases holding it and the instances registered for
//!   it.
//! - `rules::LOCATION_IS_CHILD_PARENT` (`--location-parents`):
//!   `Location.IsChild(other)` answers whether the chain of `other`'s
//!   parent locations (`PNAM`) reaches the receiver.
use crate::{
    data::key_record,
    fight::Raised,
    host::{NativeCall, Reply},
    player, rules,
    world::{ObjId, World},
    Val, Vm,
};
use fo4_core::Budget;
use fo4_plugin::{typed::Store, FormKey};

/// The event an item's container change raises.
pub const ON_CONTAINER_CHANGED: &str = "oncontainerchanged";
/// The event an actor's location change raises.
pub const ON_LOCATION_CHANGE: &str = "onlocationchange";

/// The two readings (in `crate::runstate::RunState`).
#[derive(Debug, Default, Clone, Copy)]
pub struct Switches {
    /// `rules::CONTAINER_CHANGED_EVENT` (`--container-change-events`).
    pub container: bool,
    /// `rules::LOCATION_CHANGE_EVENT` (`--location-change-events`).
    pub location: bool,
    /// `rules::LOCATION_IS_CHILD_PARENT` (`--location-parents`).
    pub parents: bool,
}
impl Switches {
    pub fn any(&self) -> bool {
        self.container || self.location || self.parents
    }
}

/// The native [`native`] answers under `rules::LOCATION_IS_CHILD_PARENT`.
pub const IS_CHILD: &str = "location.ischild";
/// The longest chain of parent locations followed.
pub const MAX_PARENT_LINKS: usize = 32;

/// A location record's parent (`PNAM`, a FormID resolved through the
/// record's plugin): `Ok(None)` when it names none.
pub fn parent_location(
    store: &Store,
    max: u64,
    k: &FormKey,
) -> Result<Option<FormKey>, &'static str> {
    let b = Budget::new(max);
    let Some((r, plugin)) = store
        .raw_with_plugin(k, &b)
        .map_err(|_| "the location record could not be read")?
    else {
        return Err("no winning record");
    };
    if r.signature != *b"LCTN" {
        return Err("not a location record");
    }
    let Some(s) = r.subrecords.iter().find(|s| s.signature == *b"PNAM") else {
        return Ok(None);
    };
    let d = r.data(s);
    if d.len() != 4 {
        return Err("a parent word of another size");
    }
    let w = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
    store
        .order()
        .remap(plugin, w, &r.source.file, s.offset)
        .map_err(|_| "the parent word names no plugin")
}

/// Whether the chain of `child`'s parent locations reaches `parent`
/// (`child` itself not counted); `Err` when a link cannot be read or the
/// chain runs past [`MAX_PARENT_LINKS`].
pub fn is_child(
    store: &Store,
    max: u64,
    parent: &FormKey,
    child: &FormKey,
) -> Result<bool, &'static str> {
    let mut at = child.clone();
    for _ in 0..MAX_PARENT_LINKS {
        match parent_location(store, max, &at)? {
            None => return Ok(false),
            Some(p) if &p == parent => return Ok(true),
            Some(p) => at = p,
        }
    }
    Err("the chain of parents is longer than the run follows")
}

fn object_key(w: &World, v: Option<&Val>) -> Option<FormKey> {
    match v {
        Some(Val::Object(o)) => w.objects.get(*o)?.key.as_deref().and_then(key_record),
        _ => None,
    }
}

/// Answers `Location.IsChild` under `rules::LOCATION_IS_CHILD_PARENT`;
/// `None` with the reading off (recorded as before).
pub fn native(store: &Store, max: u64, c: &NativeCall, w: &mut World) -> Option<Reply> {
    if !w.runstate.chain.parents || c.full_name() != IS_CHILD {
        return None;
    }
    let (Some(me), Some(other)) = (
        object_key(w, c.receiver.as_ref()),
        object_key(w, c.args.first()),
    ) else {
        w.runstate
            .count("location parents	not answered: a location is none or no record");
        return Some(Reply::Undetermined);
    };
    w.runstate.using(rules::LOCATION_IS_CHILD_PARENT);
    match is_child(store, max, &me, &other) {
        Ok(v) => {
            w.runstate.count(format!("location parents	answered {v}"));
            Some(Reply::Value(Val::Bool(v)))
        }
        Err(why) => {
            w.runstate
                .count(format!("location parents	not answered: {why}"));
            Some(Reply::Undetermined)
        }
    }
}

fn now(vm: &Vm<'_>) -> f64 {
    vm.world.now[crate::world::Clock::Real as usize]
}

fn label(vm: &Vm<'_>, o: Option<ObjId>) -> String {
    o.map(|o| vm.world.objects[o].label.clone())
        .unwrap_or_else(|| "none".into())
}

fn arg(o: Option<ObjId>) -> Val {
    o.map(Val::Object).unwrap_or(Val::None)
}

/// The caller's container change: `item` (a placed reference) moves from
/// `old` to `new` (each a holder's handle or none). Under
/// `rules::CONTAINER_CHANGED_EVENT` `OnContainerChanged(new, old)` is
/// delivered to the item's receivers.
pub fn container_changed(
    vm: &mut Vm<'_>,
    store: &Store,
    item: ObjId,
    new: Option<ObjId>,
    old: Option<ObjId>,
    b: &Budget,
) -> Result<Raised, Box<dyn std::error::Error>> {
    let t = now(vm);
    let what = format!(
        "{} moved from {} into {}",
        vm.world.objects[item].label,
        label(vm, old),
        label(vm, new)
    );
    vm.world.runstate.log.push((t, what.clone()));
    vm.world.runstate.count("container changes by the caller");
    if !vm.world.runstate.chain.container {
        return Ok(Raised {
            t,
            what,
            runs: Vec::new(),
        });
    }
    vm.rules.insert(rules::CONTAINER_CHANGED_EVENT);
    let (runs, _) = player::deliver_event(
        vm,
        store,
        item,
        ON_CONTAINER_CHANGED,
        vec![arg(new), arg(old)],
        b,
    )?;
    Ok(Raised { t, what, runs })
}

/// The caller's location change: `actor` (the player or a placed actor)
/// leaves `old` for `new` (each a location's handle or none). Under
/// `rules::LOCATION_CHANGE_EVENT` `OnLocationChange(old, new)` is
/// delivered to the actor's receivers.
pub fn location_changed(
    vm: &mut Vm<'_>,
    store: &Store,
    actor: ObjId,
    old: Option<ObjId>,
    new: Option<ObjId>,
    b: &Budget,
) -> Result<Raised, Box<dyn std::error::Error>> {
    let t = now(vm);
    let what = format!(
        "{} left {} for {}",
        vm.world.objects[actor].label,
        label(vm, old),
        label(vm, new)
    );
    vm.world.runstate.log.push((t, what.clone()));
    vm.world.runstate.count("location changes by the caller");
    if !vm.world.runstate.chain.location {
        return Ok(Raised {
            t,
            what,
            runs: Vec::new(),
        });
    }
    vm.rules.insert(rules::LOCATION_CHANGE_EVENT);
    let (runs, _) = player::deliver_event(
        vm,
        store,
        actor,
        ON_LOCATION_CHANGE,
        vec![arg(old), arg(new)],
        b,
    )?;
    Ok(Raised { t, what, runs })
}
