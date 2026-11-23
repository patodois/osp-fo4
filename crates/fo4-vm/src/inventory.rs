//! Items added to a holder from outside any script (`spec/vm.md`, Past the
//! museum): the run keeps no inventory; the caller assumes that the player
//! puts an item into a holder (`quest_run --assume-item-added`), and the
//! readings below say what that raises and what the holder's count
//! answers afterwards.
//!
//! Each reading is a pending rule behind its own switch, off by default:
//! `rules::ITEM_ADDED_EVENT` (`--item-added-events`: inventory event
//! filters kept as the run's scripts add and remove them, and the caller's
//! item delivered as `OnItemAdded` to the holder's instances whose filter
//! names the item), `rules::INVENTORY_FILTER_INIT`
//! (`--inventory-filter-init`: an init handler the run never delivers that
//! adds a filter on itself counts as having added it) and
//! `rules::ITEM_ADDED_COUNT` (`--item-added-counts`: the holder's count
//! the caller assumed grows by the item's count). Nothing is measured in
//! the running game.
use crate::{
    data::key_record,
    fight::Raised,
    host::{NativeCall, Reply},
    player::{self, Delivery, Receiver},
    rules,
    world::{InstId, ObjId, Val, World},
    Lit, Outcome, Vm, VmError,
};
use fo4_core::Budget;
use fo4_plugin::typed::Store;
use std::collections::BTreeSet;

/// The filter natives [`native`] answers under `rules::ITEM_ADDED_EVENT`.
pub const NATIVES: [&str; 3] = [
    "scriptobject.addinventoryeventfilter",
    "scriptobject.removeinventoryeventfilter",
    "scriptobject.removeallinventoryeventfilters",
];

/// The event an item added raises.
pub const ON_ITEM_ADDED: &str = "onitemadded";

/// The init handlers the init reading reads.
pub const INIT_HANDLERS: [&str; 2] = ["onaliasinit", "oninit"];

/// The run's inventory readings and the filters it keeps (in
/// `crate::runstate::RunState`).
#[derive(Debug, Default)]
pub struct Inventory {
    /// `rules::ITEM_ADDED_EVENT` (`--item-added-events`).
    pub events: bool,
    /// `rules::INVENTORY_FILTER_INIT` (`--inventory-filter-init`).
    pub filter_init: bool,
    /// `rules::ITEM_ADDED_COUNT` (`--item-added-counts`).
    pub counts_added: bool,
    /// Filters kept: (instance, the filter's value), in the order added.
    pub filters: Vec<(InstId, Val)>,
    /// Instances the init reading looked at (once each).
    pub init_read: BTreeSet<InstId>,
}
impl Inventory {
    pub fn any(&self) -> bool {
        self.events || self.counts_added
    }
}

/// Answers the filter natives under `rules::ITEM_ADDED_EVENT`: the calling
/// instance keeps or drops the filter; `None` with the reading off (the
/// call is recorded as before).
pub fn native(c: &NativeCall, w: &mut World) -> Option<Reply> {
    let full = c.full_name();
    if !w.runstate.inventory.events || !NATIVES.contains(&full.as_str()) {
        return None;
    }
    let Some(inst) = c.instance else {
        w.runstate
            .count("inventory filters\tnot kept: the calling instance is undetermined");
        return Some(Reply::Value(Val::None));
    };
    let inv = &mut w.runstate.inventory;
    match full.as_str() {
        "scriptobject.addinventoryeventfilter" => {
            let v = c.args.first().cloned().unwrap_or(Val::None);
            if !matches!(v, Val::None | Val::Object(_)) {
                w.runstate
                    .count("inventory filters\tnot kept: the filter is undetermined");
                return Some(Reply::Value(Val::None));
            }
            if !inv.filters.contains(&(inst, v.clone())) {
                inv.filters.push((inst, v));
            }
            w.runstate.count("inventory filters\tkept");
        }
        "scriptobject.removeinventoryeventfilter" => {
            let v = c.args.first().cloned().unwrap_or(Val::None);
            inv.filters.retain(|(i, f)| !(*i == inst && *f == v));
            w.runstate.count("inventory filters\tremoved");
        }
        _ => {
            inv.filters.retain(|(i, _)| *i != inst);
            w.runstate
                .count("inventory filters\tall of an instance removed");
        }
    }
    w.runstate.using(rules::ITEM_ADDED_EVENT);
    Some(Reply::Value(Val::None))
}

/// The value an init handler of the instance passes to
/// `AddInventoryEventFilter` called on itself: the argument is a script
/// variable, or a local the handler copied from one (opcode 14), read from
/// the instance now; `None` when no init handler makes such a call or the
/// argument is something else.
fn init_filter(vm: &mut Vm<'_>, inst: InstId) -> Option<Val> {
    for h in INIT_HANDLERS {
        let Ok(Some(f)) = vm.find_handler(inst, h) else {
            continue;
        };
        let mut copied: std::collections::BTreeMap<&str, &str> = Default::default();
        for i in &f.code {
            if i.op == 14 {
                if let (Some(Lit::Ident(d)), Some(Lit::Ident(s))) = (i.args.first(), i.args.get(1))
                {
                    copied.insert(d.as_str(), s.as_str());
                }
            }
            if i.op == 23
                && matches!(i.args.first(), Some(Lit::Ident(n)) if n == "addinventoryeventfilter")
                && matches!(i.args.get(1), Some(Lit::Ident(r)) if r == "self")
            {
                let name = match i.extra.first() {
                    Some(Lit::Ident(a)) => copied.get(a.as_str()).copied().unwrap_or(a.as_str()),
                    Some(Lit::None) => return Some(Val::None),
                    _ => return None,
                };
                return vm.world.instances[inst]
                    .vars
                    .iter()
                    .find(|v| v.name == name)
                    .map(|v| v.value.clone());
            }
        }
    }
    None
}

/// The objects that stand for a holder's receivers: the holder (a placed
/// reference) and the alias objects of the aliases holding it, or, for the
/// player's handle, the aliases holding the player.
fn receiver_objects(
    vm: &mut Vm<'_>,
    store: &Store,
    holder: ObjId,
    b: &Budget,
) -> Result<Vec<(ObjId, Receiver)>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    let (aliases, open) = if Some(holder) == vm.world.player {
        player::player_holders(vm, store)
    } else {
        let key = vm.world.objects[holder]
            .key
            .as_deref()
            .and_then(key_record)
            .ok_or("the holder is neither the player nor a record")?;
        out.push((holder, Receiver::Reference));
        player::holders(vm, store, &key)
    };
    if !aliases.is_empty() || open > 0 {
        vm.rules.insert(rules::EVENT_ALIASES);
    }
    for (q, id) in aliases {
        let qo = crate::data::record_object(&mut vm.world, store, &q, "quest", b)?;
        let label = format!("alias {id} of {}", vm.world.objects[qo].label);
        let akey = format!("{}#alias:{id}", crate::bind::record_key(&q));
        let a = vm.world.keyed_object(&label, &akey, "referencealias");
        out.push((a, Receiver::Alias(qo, id as i32)));
    }
    Ok(out)
}

fn now(vm: &Vm<'_>) -> f64 {
    vm.world.now[crate::world::Clock::Real as usize]
}

/// The caller's item added (`quest_run --assume-item-added`): `count` of
/// `item` (a base form's handle) put into `holder` from `source` (none, or
/// the player). Under `rules::ITEM_ADDED_COUNT` the holder's count the
/// caller assumed grows by `count`; under `rules::ITEM_ADDED_EVENT`
/// `OnItemAdded(item, count, none, source)` runs on each receiver instance
/// whose kept filter is the item or none (with
/// `rules::INVENTORY_FILTER_INIT`, the filter its init handler adds
/// counts as kept).
pub fn item_added(
    vm: &mut Vm<'_>,
    store: &Store,
    holder: ObjId,
    item: ObjId,
    count: i32,
    source: Option<ObjId>,
    b: &Budget,
) -> Result<Raised, Box<dyn std::error::Error>> {
    let t = now(vm);
    let what = format!(
        "{count} of {} put into {}",
        vm.world.objects[item].label, vm.world.objects[holder].label
    );
    vm.world.runstate.log.push((t, what.clone()));
    vm.world.runstate.count("items added by the caller");
    if vm.world.runstate.inventory.counts_added {
        let hk = if Some(holder) == vm.world.player {
            Some("player".to_string())
        } else {
            vm.world.objects[holder].key.clone()
        };
        let ik = vm.world.objects[item].key.clone();
        match (hk, ik) {
            (Some(h), Some(i)) => match vm.world.runstate.item_counts.get_mut(&(h, i)) {
                Some(n) => {
                    *n += count;
                    let n = *n;
                    vm.rules.insert(rules::ITEM_ADDED_COUNT);
                    vm.world.runstate.log.push((t, format!("count now {n}")));
                    vm.world
                        .runstate
                        .count("item counts\traised by an item added");
                }
                None => vm
                    .world
                    .runstate
                    .count("item counts\tnot raised: no count known before"),
            },
            _ => vm
                .world
                .runstate
                .count("item counts\tnot raised: the holder or the item is not a record"),
        }
    }
    let mut runs = Vec::new();
    if !vm.world.runstate.inventory.events {
        return Ok(Raised { t, what, runs });
    }
    vm.rules.insert(rules::ITEM_ADDED_EVENT);
    let args = vec![
        Val::Object(item),
        Val::Int(count),
        Val::None,
        source.map(Val::Object).unwrap_or(Val::None),
    ];
    for (o, receiver) in receiver_objects(vm, store, holder, b)? {
        if let Err(e) = vm.attach_scripts(o) {
            runs.push(Delivery {
                receiver,
                script: String::new(),
                outcome: Outcome::Failed(e),
            });
            continue;
        }
        for inst in vm.world.objects[o].instances.clone() {
            let script = vm.world.instances[inst].script.clone();
            let func = match vm.find_handler(inst, ON_ITEM_ADDED) {
                Ok(Some(f)) => f,
                Ok(None) => continue,
                Err(e) => {
                    runs.push(Delivery {
                        receiver: receiver.clone(),
                        script,
                        outcome: Outcome::Failed(e),
                    });
                    continue;
                }
            };
            if vm.world.runstate.inventory.filter_init
                && vm.world.runstate.inventory.init_read.insert(inst)
            {
                if let Some(v) = init_filter(vm, inst) {
                    vm.rules.insert(rules::INVENTORY_FILTER_INIT);
                    let inv = &mut vm.world.runstate.inventory;
                    if !inv.filters.contains(&(inst, v.clone())) {
                        inv.filters.push((inst, v));
                    }
                    vm.world.runstate.count(
                        "inventory filters\tread from an init handler the run never delivered",
                    );
                }
            }
            let filters: Vec<Val> = vm
                .world
                .runstate
                .inventory
                .filters
                .iter()
                .filter(|(i, _)| *i == inst)
                .map(|(_, f)| f.clone())
                .collect();
            if filters.is_empty() {
                vm.world
                    .runstate
                    .count("items added\tnot delivered: the instance keeps no filter");
                continue;
            }
            if !filters
                .iter()
                .any(|f| matches!(f, Val::None) || *f == Val::Object(item))
            {
                vm.world.runstate.count(
                    "items added\tnot delivered: no filter the instance keeps names the item",
                );
                continue;
            }
            let outcome = if func.params.len() == args.len() {
                vm.world.runstate.count("items added\tdelivered");
                vm.deliver(inst, ON_ITEM_ADDED, args.clone())
            } else {
                Outcome::Failed(VmError::BadArgument {
                    site: None,
                    detail: "event onitemadded takes parameters the event does not give".into(),
                })
            };
            runs.push(Delivery {
                receiver: receiver.clone(),
                script,
                outcome,
            });
        }
    }
    Ok(Raised { t, what, runs })
}
