//! The player's pick of a terminal's menu item (`spec/vm.md`, Past the
//! museum): the run opens no menu; the caller assumes that the player
//! picks the item of an id on a placed terminal
//! (`quest_run --assume-terminal-item REF ID`), and under
//! `rules::TERMINAL_FRAGMENT` (`--terminal-fragments`, off by default) the
//! function the terminal record's fragment block names for that id runs on
//! the block's script instance (`vmad_fragment_instance_v1`), with the
//! placed terminal as its one argument. The item's conditions, its
//! submenu and anything else the menu shows are not read. Nothing is
//! measured in the running game.
use crate::{
    bind,
    data::{key_record, record_object},
    fight::Raised,
    player::{Delivery, Receiver},
    rules,
    world::{ObjId, Val},
    Outcome, Vm,
};
use fo4_core::Budget;
use fo4_plugin::{typed::Store, vmad::Vmad};

/// The function a terminal record's fragment block names for an item id,
/// lower case; `None` when the record has no block or no entry of that
/// index.
pub fn fragment_function(
    store: &Store,
    terminal: &fo4_plugin::FormKey,
    item: u32,
    b: &Budget,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let Some((r, _)) = bind::vmad_of(store, terminal, b)? else {
        return Ok(None);
    };
    for s in r.subrecords.iter().filter(|s| s.signature == *b"VMAD") {
        let v = Vmad::read(r.data(s), r.signature, &r.source.file, s.offset, b)?;
        if let Some(t) = &v.terminal {
            if let Some(f) = t.fragments.iter().find(|f| f.index == item) {
                return Ok(Some(fo4_pex::index::lower(f.function)));
            }
        }
    }
    Ok(None)
}

/// The caller's pick of item `item` on the placed terminal `reference`.
/// With `rules::TERMINAL_FRAGMENT` off nothing runs (the pick is logged).
pub fn pick(
    vm: &mut Vm<'_>,
    store: &Store,
    reference: ObjId,
    item: u32,
    b: &Budget,
) -> Result<Raised, Box<dyn std::error::Error>> {
    let t = vm.world.now[crate::world::Clock::Real as usize];
    let what = format!(
        "the player picks item {item} on {}",
        vm.world.objects[reference].label
    );
    vm.world.runstate.log.push((t, what.clone()));
    vm.world
        .runstate
        .count("terminal items picked by the caller");
    let mut runs = Vec::new();
    if !vm.world.runstate.terminals {
        return Ok(Raised { t, what, runs });
    }
    let key = vm.world.objects[reference]
        .key
        .as_deref()
        .and_then(key_record)
        .ok_or("the terminal is not a record")?;
    let base = bind::base_of(store, &key, b)?.ok_or("the terminal reference has no base")?;
    if store.signature(&base) != Some(*b"TERM") {
        return Err("the reference's base is not a terminal (TERM)".into());
    }
    let Some(function) = fragment_function(store, &base, item, b)? else {
        vm.world
            .runstate
            .count("terminal items\tno fragment of the item's id");
        return Ok(Raised { t, what, runs });
    };
    vm.rules.insert(rules::TERMINAL_FRAGMENT);
    let form = record_object(&mut vm.world, store, &base, "terminal", b)?;
    let mut notes = Vec::new();
    let outcome = match crate::scene::record_fragment_instance(vm, store, form, &mut notes, b) {
        Ok(inst) => {
            let script = vm.world.instances[inst].script.clone();
            vm.world.runstate.count("terminal items\tfragment run");
            let o = vm.deliver(inst, &function, vec![Val::Object(reference)]);
            runs.push(Delivery {
                receiver: Receiver::Reference,
                script,
                outcome: o,
            });
            None
        }
        Err(e) => Some(e),
    };
    if let Some(e) = outcome {
        runs.push(Delivery {
            receiver: Receiver::Reference,
            script: String::new(),
            outcome: Outcome::Failed(e),
        });
    }
    Ok(Raised { t, what, runs })
}
