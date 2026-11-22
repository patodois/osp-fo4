//! `--script-event INDEX:EVENT[,EVENT...]`: runs the scripts of one placed
//! reference of the cell with `fo4_vm` (data host, every rule pending)
//! before the frame is drawn, prints the effect log, and applies only the
//! visibility changes the log names on references of the loaded cells
//! (`vm_disable_hides_reference_v1`): a disable takes a drawn reference out
//! of the scene, an enable of an initially disabled one puts it in, made
//! as the cell load makes an active one. Every other native stays log-only.
//!
//! The interpreter is linked only with `--features script-event`; without
//! the feature the option is refused and the build links nothing new
//! (spec/vm.md, Visible effect).
#[cfg(feature = "script-event")]
use crate::load::AnyResult;
#[cfg(feature = "script-event")]
use fo4_core::Budget;
#[cfg(feature = "script-event")]
use fo4_scene::RuleLevel;
#[cfg(feature = "script-event")]
use fo4_vm::bind::{self, Event, VfsLibrary};
#[cfg(feature = "script-event")]
use std::collections::BTreeMap;

/// What `--script-event` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptEvent {
    /// Index into the cell's placed references as `script_run CELL list`
    /// numbers them (`fo4_vm::bind::placed`).
    pub index: usize,
    /// Event names (`fo4_vm::bind::EVENTS`, `fragment:N`), delivered in order.
    pub events: Vec<String>,
}

#[cfg(not(feature = "script-event"))]
pub fn parse(_: &str) -> Result<ScriptEvent, String> {
    Err("--script-event needs a viewer built with --features script-event".into())
}

#[cfg(feature = "script-event")]
pub fn parse(text: &str) -> Result<ScriptEvent, String> {
    let bad = || format!("ExpectedScriptEvent INDEX:EVENT[,EVENT...]: {text}");
    let (i, e) = text.split_once(':').ok_or_else(bad)?;
    let index = i.parse::<usize>().map_err(|_| bad())?;
    let events: Vec<String> = e.split(',').map(str::to_string).collect();
    if events.iter().any(|x| Event::parse(x).is_none()) {
        return Err(bad());
    }
    Ok(ScriptEvent { index, events })
}

/// The rules a script event relies on before any is run.
#[cfg(feature = "script-event")]
pub fn rules() -> Vec<&'static str> {
    fo4_vm::rules::ALL.to_vec()
}

#[cfg(feature = "script-event")]
/// Runs the event; in `scene`, takes out the references whose last
/// visibility change in the effect log is a disable and puts in the
/// initially disabled references whose last change is an enable.
#[allow(clippy::too_many_arguments)]
pub fn apply(
    ev: &ScriptEvent,
    store: &fo4_plugin::typed::Store,
    vfs: &fo4_vfs::Vfs,
    max_memory: u64,
    scene: &mut fo4_scene::Scene,
    loader: &mut fo4_scene::Loader<'_>,
    budget: &Budget,
    level: RuleLevel,
) -> AnyResult<()> {
    if level != RuleLevel::Hypotheses {
        return Err(format!(
            "ScriptEventRuleLevel: --script-event relies on the pending rules {:?} (--under-hypotheses)",
            rules()
        )
        .into());
    }
    let b = Budget::new(max_memory);
    let refs = bind::placed(store, &scene.cell.key, &b)?;
    let r = refs.get(ev.index).ok_or_else(|| {
        format!(
            "ScriptEventIndex: {} of {} placed references",
            ev.index,
            refs.len()
        )
    })?;
    let base = bind::base_of(store, r, &b)?.ok_or("ScriptEventReference: no base record")?;
    let events: Vec<Event> = ev.events.iter().filter_map(|e| Event::parse(e)).collect();
    let mut lib = VfsLibrary::new(vfs, max_memory);
    let run = bind::run(&mut lib, store, Some(r), &base, &events, 0, &b)?;
    for line in bind::effect_log(&run).lines() {
        println!("script-event: {line}");
    }
    // The last change per reference decides.
    let mut last: BTreeMap<String, bool> = BTreeMap::new();
    for (k, shown) in bind::visibility_changes(&run) {
        last.insert(k, shown);
    }
    for (k, shown) in last {
        let at = scene
            .instances
            .iter()
            .position(|i| bind::record_key(&i.reference.key) == k);
        match (shown, at) {
            (false, Some(i)) => {
                let gone = scene.instances.remove(i);
                let causes: Vec<&str> = gone.blockers.iter().map(|e| e.code.as_str()).collect();
                println!(
                    "script-event: {k} hidden under {} (scene instance {i}; drawable at this level before: {}{})",
                    fo4_vm::rules::DISABLE_HIDES,
                    gone.state.ready_at(level),
                    if causes.is_empty() {
                        String::new()
                    } else {
                        format!("; blockers {causes:?}")
                    }
                );
            }
            (false, None) => {
                println!("script-event: {k} disabled, but it is not a drawn instance of the loaded cells")
            }
            (true, Some(_)) => println!("script-event: {k} enabled; it is already drawn"),
            (true, None) => {
                let Some(d) = scene
                    .disabled
                    .iter()
                    .position(|c| bind::record_key(&c.key) == k)
                else {
                    println!(
                        "script-event: {k} enabled, but it is neither drawn nor an initially disabled reference of the loaded cells"
                    );
                    continue;
                };
                let child = scene.disabled.remove(d);
                let scope = budget.child();
                let instance = loader.instance(&child.key, &scope)?;
                scope.commit(budget)?;
                let causes: Vec<&str> = instance.blockers.iter().map(|e| e.code.as_str()).collect();
                println!(
                    "script-event: {k} initially disabled, enabled: shown under {} (scene instance {}; drawable at this level: {}{})",
                    fo4_vm::rules::DISABLE_HIDES,
                    scene.instances.len(),
                    instance.state.ready_at(level),
                    if causes.is_empty() {
                        String::new()
                    } else {
                        format!("; blockers {causes:?}")
                    }
                );
                scene.assets.extend_from_slice(&instance.assets);
                scene.assets.sort_unstable();
                scene.assets.dedup();
                scene.instances.push(instance);
            }
        }
    }
    Ok(())
}

#[cfg(all(test, feature = "script-event"))]
mod tests {
    use super::*;

    #[test]
    fn script_events_parse_and_refuse_unknown_names() {
        assert_eq!(
            parse("77:activate").unwrap(),
            ScriptEvent {
                index: 77,
                events: vec!["activate".into()]
            }
        );
        assert_eq!(parse("3:load,activate").unwrap().events.len(), 2);
        assert!(parse("3:fragment:2").is_ok());
        assert!(parse("activate").is_err());
        assert!(parse("x:activate").is_err());
        assert!(parse("3:jump").is_err());
    }
}
