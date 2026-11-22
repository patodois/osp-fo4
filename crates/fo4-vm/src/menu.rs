//! Menus (`spec/vm.md`, Menus): registrations for menu events kept, and
//! the events delivered where a caller assumes a menu opens or closes
//! (`quest_run --assume-menu NAME open|close`) or the appearance menu
//! raises its event (`--assume-looks-menu-event N`).
//!
//! The interpreter has no interface: it opens no menu and does nothing a
//! menu does to the player record. The menu names come from the scripts'
//! registrations and from the caller; none is stored here. Every behaviour
//! is one pending rule, `rules::MENU_EVENTS`, behind its switch
//! (`quest_run --menu-events`), off by default: with it off the
//! registration natives are recorded as any other native, as before.
use crate::{
    host::{NativeCall, Reply},
    rules,
    world::{InstId, Val, World},
    Outcome, Vm, VmError,
};
use std::collections::BTreeMap;

/// The registrations of a run (kept in [`World`]).
#[derive(Debug, Default)]
pub struct Menus {
    /// Keep registrations and deliver the caller's menu events
    /// (`rules::MENU_EVENTS`, `quest_run --menu-events`).
    pub on: bool,
    /// (instance, menu name as the call stored it), in registration order.
    pub open_close: Vec<(InstId, String)>,
    /// Instances registered for the appearance menu's event.
    pub looks: Vec<InstId>,
    pub counts: BTreeMap<String, u64>,
}
impl Menus {
    pub fn count(&mut self, k: impl Into<String>) {
        *self.counts.entry(k.into()).or_default() += 1;
    }
}

/// The registration natives.
pub const NATIVES: [&str; 5] = [
    "scriptobject.registerformenuopencloseevent",
    "scriptobject.unregisterformenuopencloseevent",
    "scriptobject.unregisterforallmenuopencloseevents",
    "scriptobject.registerforlooksmenuevent",
    "scriptobject.unregisterforlooksmenuevent",
];

/// The registration natives when [`Menus::on`]: kept, answered none.
pub fn native(c: &NativeCall, w: &mut World) -> Option<Reply> {
    let full = c.full_name();
    if !w.menus.on || !NATIVES.contains(&full.as_str()) {
        return None;
    }
    let Some(i) = c.instance else {
        w.menus
            .count("registrations\tnot read: the calling instance is undetermined");
        return Some(Reply::Value(Val::None));
    };
    let name = match c.args.first() {
        Some(Val::Str(s)) => Some(s.clone()),
        _ => None,
    };
    match (full.as_str(), name) {
        ("scriptobject.registerformenuopencloseevent", Some(n)) => {
            w.menus.count("registrations\tmenu open and close: made");
            if !w.menus.open_close.contains(&(i, n.clone())) {
                w.menus.open_close.push((i, n));
            }
        }
        ("scriptobject.unregisterformenuopencloseevent", Some(n)) => {
            w.menus.count("registrations\tmenu open and close: removed");
            w.menus
                .open_close
                .retain(|(x, m)| !(*x == i && m.eq_ignore_ascii_case(&n)));
        }
        ("scriptobject.unregisterforallmenuopencloseevents", _) => {
            w.menus
                .count("registrations\tmenu open and close: all removed");
            w.menus.open_close.retain(|(x, _)| *x != i);
        }
        ("scriptobject.registerforlooksmenuevent", _) => {
            w.menus.count("registrations\tappearance menu: made");
            if !w.menus.looks.contains(&i) {
                w.menus.looks.push(i);
            }
        }
        ("scriptobject.unregisterforlooksmenuevent", _) => {
            w.menus.count("registrations\tappearance menu: removed");
            w.menus.looks.retain(|x| *x != i);
        }
        _ => w
            .menus
            .count("registrations\tnot read: the menu name is undetermined"),
    }
    Some(Reply::Value(Val::None))
}

fn run(vm: &mut Vm<'_>, i: InstId, handler: &str, args: Vec<Val>) -> Option<(String, Outcome)> {
    let script = vm.world.instances[i].script.clone();
    let n = args.len();
    Some(match vm.find_handler(i, handler) {
        Ok(Some(f)) if f.params.len() == n => {
            vm.world.menus.count("deliveries\ta handler ran");
            (script, vm.deliver(i, handler, args))
        }
        Ok(Some(_)) => (
            script,
            Outcome::Failed(VmError::BadArgument {
                site: None,
                detail: format!("{handler} takes parameters the event does not give"),
            }),
        ),
        Ok(None) => {
            vm.world
                .menus
                .count("deliveries\tthe registered instance has no handler");
            return None;
        }
        Err(e) => (script, Outcome::Failed(e)),
    })
}

/// A menu of that name opens (`opening`) or closes: `OnMenuOpenCloseEvent(
/// name, opening)` on every instance registered for that name (names
/// compared ignoring ASCII case; the handler gets the registration's
/// spelling), in registration order.
pub fn open_close(vm: &mut Vm<'_>, name: &str, opening: bool) -> Vec<(String, Outcome)> {
    if !vm.world.menus.on {
        return Vec::new();
    }
    vm.rules.insert(rules::MENU_EVENTS);
    let regs: Vec<(InstId, String)> = vm
        .world
        .menus
        .open_close
        .iter()
        .filter(|(_, n)| n.eq_ignore_ascii_case(name))
        .cloned()
        .collect();
    regs.into_iter()
        .filter_map(|(i, n)| {
            run(
                vm,
                i,
                "onmenuopencloseevent",
                vec![Val::Str(n), Val::Bool(opening)],
            )
        })
        .collect()
}

/// The appearance menu raises its event with the caller's number:
/// `OnLooksMenuEvent(flavor)` on every registered instance.
pub fn looks(vm: &mut Vm<'_>, flavor: i32) -> Vec<(String, Outcome)> {
    if !vm.world.menus.on {
        return Vec::new();
    }
    vm.rules.insert(rules::MENU_EVENTS);
    let regs = vm.world.menus.looks.clone();
    regs.into_iter()
        .filter_map(|i| run(vm, i, "onlooksmenuevent", vec![Val::Int(flavor)]))
        .collect()
}
