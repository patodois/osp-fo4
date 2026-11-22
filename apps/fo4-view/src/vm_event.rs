//! `--script-event INDEX:EVENT[,EVENT...]`: runs the scripts of one placed
//! reference of the cell with `fo4_vm` (data host, every rule pending)
//! before the frame is drawn, prints the effect log, and applies only the
//! visibility changes the log names on references of the loaded cells
//! (`vm_disable_hides_reference_v1`): a disable takes a drawn reference out
//! of the scene, an enable of an initially disabled one puts it in, made
//! as the cell load makes an active one. Every other native stays log-only.
//! The changes then reach the enable children of the changed references
//! (`xesp_script_change_propagates_v1`): every instance whose enable parent
//! chain names a changed reference takes the state its chain now gives,
//! with the opposite flag, and is drawn or left out by it.
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
) -> AnyResult<BTreeMap<String, bool>> {
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
    let changes = last.clone();
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
            (true, Some(_)) => println!(
                "script-event: {k} enabled; it is already a scene instance (drawn unless its enable parent chain says otherwise)"
            ),
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
    propagate(scene, &changes, level);
    Ok(changes)
}

/// Gives every instance whose enable parent chain names a changed
/// reference (or the instance itself) the state the chain now gives
/// (`xesp_script_change_propagates_v1`); returns how many changed. The
/// actors of the frame are resolved after this: [`propagate_actors`].
#[cfg(feature = "script-event")]
pub fn propagate(
    scene: &mut fo4_scene::Scene,
    changes: &BTreeMap<String, bool>,
    level: RuleLevel,
) -> usize {
    let set = |k: &fo4_plugin::FormKey| changes.get(&bind::record_key(k)).copied();
    let mut changed = 0;
    for (n, inst) in scene.instances.iter_mut().enumerate() {
        let Some(p) = &inst.enable else { continue };
        let reached = p
            .chain
            .nodes
            .iter()
            .any(|x| changes.contains_key(&bind::record_key(&x.key)));
        if !reached {
            continue;
        }
        let (before, after) = (p.enabled, p.with(&set));
        if before == after {
            continue;
        }
        inst.set_enabled(after);
        changed += 1;
        println!(
            "script-event: {} enable child: start {before:?}, now {after:?} under {} (scene instance {n}; drawable at this level: {})",
            bind::record_key(&inst.reference.key),
            fo4_scene::hypotheses::ENABLE_SCRIPT,
            inst.state.ready_at(level) && !inst.editor_marker && after == Some(true)
        );
    }
    // Initially disabled references with an XESP stay out: the rules do not
    // decide a record that carries its own flag beside an enable parent.
    changed
}

/// [`propagate`] for the placed actors.
#[cfg(feature = "script-event")]
pub fn propagate_actors(
    actors: &mut fo4_scene::actor::Actors,
    changes: &BTreeMap<String, bool>,
    level: RuleLevel,
) -> usize {
    let set = |k: &fo4_plugin::FormKey| changes.get(&bind::record_key(k)).copied();
    let mut changed = 0;
    for a in &mut actors.actors {
        let Some(p) = &mut a.enable else { continue };
        if !p
            .chain
            .nodes
            .iter()
            .any(|x| changes.contains_key(&bind::record_key(&x.key)))
        {
            continue;
        }
        let (before, after) = (p.enabled, p.with(&set));
        if before == after {
            continue;
        }
        p.enabled = after;
        changed += 1;
        println!(
            "script-event: {} enable child (actor): start {before:?}, now {after:?} under {} (drawable at this level: {})",
            bind::record_key(&a.reference),
            fo4_scene::hypotheses::ENABLE_SCRIPT,
            a.plan.is_some() && a.state.ready_at(level) && after == Some(true)
        );
    }
    changed
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

    /// A synthetic cell: persistent parents 0x810 (enabled) and 0x811
    /// (initially disabled); children 0x812 of 0x810, 0x813 of 0x811, 0x814
    /// of 0x811 with the opposite bit, 0x815 of 0x813 (depth 2), and 0x816
    /// without a parent. A script's change of 0x810 and 0x811 reaches the
    /// children through their chains and nothing else.
    #[test]
    fn propagate_gives_the_children_of_changed_references_their_chain_state() {
        use fo4_core::{Game, Limits, Manifest, Mount, PluginSpec, TestDir};
        fn sub(sig: &[u8; 4], d: &[u8]) -> Vec<u8> {
            let mut v = sig.to_vec();
            v.extend((d.len() as u16).to_le_bytes());
            v.extend(d);
            v
        }
        fn record(sig: &[u8; 4], id: u32, flags: u32, d: &[u8]) -> Vec<u8> {
            let mut v = sig.to_vec();
            for n in [d.len() as u32, flags, id, 0] {
                v.extend(n.to_le_bytes());
            }
            v.extend(131u16.to_le_bytes());
            v.extend(0u16.to_le_bytes());
            v.extend(d);
            v
        }
        fn group(kind: u32, id: u32, d: &[u8]) -> Vec<u8> {
            let mut v = b"GRUP".to_vec();
            v.extend((d.len() as u32 + 24).to_le_bytes());
            v.extend(id.to_le_bytes());
            v.extend(kind.to_le_bytes());
            v.extend([0; 8]);
            v.extend(d);
            v
        }
        let mut rf = sub(b"NAME", &0x801u32.to_le_bytes());
        rf.extend(sub(b"DATA", &[0; 24]));
        let child = |id: u32, parent: Option<(u32, u8)>| {
            let mut v = rf.clone();
            if let Some((p, flags)) = parent {
                let mut d = p.to_le_bytes().to_vec();
                d.extend([flags, 0, 0, 0]);
                v.extend(sub(b"XESP", &d));
            }
            record(b"REFR", id, 0, &v)
        };
        let persistent = [
            record(b"REFR", 0x810, 0, &rf),
            record(b"REFR", 0x811, fo4_plugin::DISABLED, &rf),
        ]
        .concat();
        let temporary = [
            child(0x812, Some((0x810, 0))),
            child(0x813, Some((0x811, 0))),
            child(0x814, Some((0x811, 1))),
            child(0x815, Some((0x813, 0))),
            child(0x816, None),
        ]
        .concat();
        let mut body = group(
            2,
            0,
            &[
                record(b"CELL", 0x800, 0, &sub(b"EDID", b"Room\0")),
                group(
                    6,
                    0x800,
                    &[group(8, 0x800, &persistent), group(9, 0x800, &temporary)].concat(),
                ),
            ]
            .concat(),
        );
        body.extend(record(b"STAT", 0x801, 0, &sub(b"EDID", b"Box\0")));
        let mut hedr = 0.95f32.to_le_bytes().to_vec();
        hedr.extend(0u32.to_le_bytes());
        hedr.extend(0x900u32.to_le_bytes());
        let mut plugin = record(b"TES4", 0, 1, &sub(b"HEDR", &hedr));
        plugin.extend(body);
        let temp = TestDir::new("view-propagate");
        let root = temp.join("Data");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("base.esm"), plugin).unwrap();
        let manifest = Manifest {
            game: Game::Fallout4,
            archive_invalidation: None,
            root,
            language: "english".into(),
            plugins: vec![PluginSpec {
                name: "base.esm".into(),
                path: "base.esm".into(),
            }],
            mounts: vec![Mount::Loose {
                plugin: None,
                base: None,
                files: vec![],
            }],
            required_masters: vec![],
            excluded: vec![],
            build: Some("synthetic".into()),
            limits: Limits::default(),
        };
        let store = fo4_plugin::typed::Store::open(&manifest, &Budget::new(8 << 20)).unwrap();
        let vfs = fo4_vfs::Vfs::mount(&manifest, &Budget::new(8 << 20)).unwrap();
        let cache = Budget::new(8 << 20);
        let mut loader =
            fo4_scene::Loader::new(&store, &vfs, "english", RuleLevel::Hypotheses, &cache);
        let mut scene = loader.by_editor_id("Room", &Budget::new(8 << 20)).unwrap();
        let state = |s: &fo4_scene::Scene, id: u32| {
            s.instances
                .iter()
                .find(|i| i.reference.key.object_id == id)
                .and_then(|i| i.enable.as_ref())
                .map(|p| p.enabled)
        };
        assert_eq!(state(&scene, 0x812), Some(Some(true)));
        assert_eq!(state(&scene, 0x813), Some(Some(false)));
        assert_eq!(state(&scene, 0x814), Some(Some(true)));
        assert_eq!(state(&scene, 0x815), Some(Some(false)));
        // Nothing changed: nothing moves.
        let none = BTreeMap::new();
        assert_eq!(propagate(&mut scene, &none, RuleLevel::Hypotheses), 0);
        // The script disables 0x810 and enables 0x811.
        let key = |id: u32| {
            bind::record_key(&fo4_plugin::FormKey {
                plugin: "base.esm".into(),
                object_id: id,
            })
        };
        let changes: BTreeMap<String, bool> = [(key(0x810), false), (key(0x811), true)].into();
        assert_eq!(propagate(&mut scene, &changes, RuleLevel::Hypotheses), 4);
        assert_eq!(state(&scene, 0x812), Some(Some(false)));
        assert_eq!(state(&scene, 0x813), Some(Some(true)));
        assert_eq!(state(&scene, 0x814), Some(Some(false)));
        assert_eq!(state(&scene, 0x815), Some(Some(true)));
        // The reference without a parent has no chain to follow.
        assert_eq!(state(&scene, 0x816), None);
        // The same changes again change nothing more.
        assert_eq!(propagate(&mut scene, &changes, RuleLevel::Hypotheses), 0);
    }
}
