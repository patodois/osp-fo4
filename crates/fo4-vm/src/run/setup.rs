//! Pieces every mode of a quest run sets up: the story tree, the quest
//! host, the world's event switches, the voice files, models' graphs, and
//! lookups by editor ID.
use super::*;

/// The story manager's nodes, read once when the story manager is on.
pub(super) fn story_tree(store: &Store, o: Options) -> Result<Option<Rc<Tree>>, E> {
    if !o.story {
        return Ok(None);
    }
    let b = Budget::new(store.limits.max_memory);
    Ok(Some(Rc::new(Tree::read(store, &b)?)))
}

pub(super) fn host<'s>(
    store: &'s Store,
    vfs: &'s Vfs,
    max: u64,
    o: Options,
    tree: &Option<Rc<Tree>>,
    settings: &Settings,
) -> QuestHost<'s> {
    let mut h = QuestHost::new(store, max);
    h.cond.switches = settings.cond.0.clone();
    h.cond.assume = settings.cond.1.clone();
    h.cond.assume_for = settings.cond_for.clone();
    if h.cond.switches.on(629) {
        h.cond.scripts = Some(std::cell::RefCell::new(Box::new(VfsLibrary::new(vfs, max))));
    }
    h.evaluate = o.conditions;
    h.stage_event = o.stage_event;
    h.data.enable_chain = o.enable_chain;
    h.data.child_flag_ignored = o.child_flag_ignored;
    if o.story {
        h.story = tree.clone();
    }
    h
}

/// Switches the world's event delivery as the options say.
pub(super) fn events_on(vm: &mut Vm<'_>, o: Options, settings: &Settings) {
    vm.world.events.remote = o.remote;
    vm.world.events.custom = o.custom;
    vm.world.scenes.on = o.scenes;
    vm.world.scenes.begin_on_quest_start = o.scenes && o.scenes_with_quest;
    vm.world.scenes.stage_fields = o.scenes && o.stage_fields;
    vm.world.scenes.action_conditions = o.scenes && o.action_conditions;
    vm.world.scenes.action_percent = o.scenes && o.action_percent;
    vm.world.scenes.conditions_end = o.scenes && o.conditions_end;
    vm.world.quests.location_fills = o.location_fills;
    vm.world.scenes.line_fields = o.scenes && o.line_fields;
    if o.scenes {
        vm.world.scenes.lines = o.lines;
    }
    vm.none_arrays = o.none_arrays;
    vm.texts_ignore_case = o.texts_ignore_case;
    vm.auto_state_inherited = o.auto_state_inherited;
    vm.property_implicit_instance = o.property_implicit_instance;
    if o.scenes {
        vm.world.scenes.groups = settings.groups.clone();
        vm.world.scenes.clock = settings.clock.clone();
    }
}

/// The voice files of the mounted archives with the seconds their headers
/// declare, read by the caller (`fo4_audio::header_seconds`,
/// `audio_xwma_dpds_seconds_v1`), and the lines of every topic.
pub(super) fn load_voice(
    m: &Manifest,
    vfs: &Vfs,
    store: &Store,
    seconds: HeaderSeconds<'_>,
) -> Result<voice::Voice, E> {
    let mut v = voice::Voice {
        topics: voice::Topics::read(store),
        ..voice::Voice::default()
    };
    let c = &vfs.candidates;
    for (i, x) in c.iter().enumerate() {
        let Some(k) = &x.key else { continue };
        let winner = c.get(i + 1).and_then(|n| n.key.as_ref()) != Some(k);
        if !winner || voice::parse_path(k).is_none() {
            continue;
        }
        let b = Budget::new(m.limits.max_payload);
        let bytes = vfs.read_candidate(x, &b)?;
        let secs = seconds(&bytes, &x.original, &b);
        v.files.add(k, secs);
    }
    Ok(v)
}

/// Reads models' graphs through the mounted archives
/// (`fo4_scene::anim_graph::model_graph`), once per model text.
pub(super) struct VfsGraphs<'a> {
    pub(super) vfs: &'a Vfs,
    pub(super) cache: BTreeMap<String, Result<Rc<fo4_scene::anim_graph::ModelGraph>, String>>,
}
impl crate::anim::GraphSource for VfsGraphs<'_> {
    fn graph(&mut self, model: &str) -> Result<Rc<fo4_scene::anim_graph::ModelGraph>, String> {
        let key = model.to_ascii_lowercase();
        if let Some(g) = self.cache.get(&key) {
            return g.clone();
        }
        let limits = self.vfs.limits().clone();
        let b = Budget::new(limits.max_memory);
        let g = fo4_scene::anim_graph::model_graph(self.vfs, model, &limits, &b).map(Rc::new);
        self.cache.insert(key, g.clone());
        g
    }
}

/// The winning NPC_ of an editor ID.
pub(super) fn npc_by_editor_id(store: &Store, b: &Budget, edid: &str) -> Result<FormKey, E> {
    Ok(store
        .winners()
        .filter(|w| !w.deleted && w.signature == *b"NPC_")
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .find(|k| {
            store
                .raw(k, &b.child())
                .ok()
                .flatten()
                .and_then(|r| bind::editor_id(&r))
                .is_some_and(|e| e.eq_ignore_ascii_case(edid))
        })
        .ok_or("no NPC_ of that editor ID")?)
}

/// The DATA of the winning game setting (GMST) of an editor ID, read as
/// an f32 (`dialogue_hello_proximity_v1` reads the hello distance so).
pub(super) fn game_setting_float(store: &Store, b: &Budget, edid: &str) -> Result<f32, E> {
    for w in store
        .winners()
        .filter(|w| !w.deleted && w.signature == *b"GMST")
    {
        let k = store.overrides().form_key(w.key, store.order());
        let Some(r) = store.raw(&k, &b.child())? else {
            continue;
        };
        if !bind::editor_id(&r).is_some_and(|e| e.eq_ignore_ascii_case(edid)) {
            continue;
        }
        let d = r
            .subrecords
            .iter()
            .find(|s| s.signature == *b"DATA")
            .map(|s| r.data(s))
            .ok_or("the game setting has no DATA")?;
        let a: [u8; 4] = d
            .try_into()
            .map_err(|_| "the game setting's DATA is not 4 bytes")?;
        let v = f32::from_le_bytes(a);
        if !v.is_finite() || v < 0.0 {
            return Err("the game setting is not a finite distance".into());
        }
        return Ok(v);
    }
    Err("no game setting of that editor ID".into())
}

pub(super) fn quest_by_editor_id(store: &Store, b: &Budget, edid: &str) -> Result<FormKey, E> {
    Ok(store
        .winners()
        .filter(|w| !w.deleted && w.signature == *b"QUST")
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .find(|k| {
            store
                .raw(k, &b.child())
                .ok()
                .flatten()
                .and_then(|r| bind::editor_id(&r))
                .is_some_and(|e| e.eq_ignore_ascii_case(edid))
        })
        .ok_or("no quest of that editor ID")?)
}

/// The GLOB whose editor ID the caller named: the role
/// `vm_timer_time_scale_v1` gives it is an assumption of the rule.
pub(super) fn time_scale_global(store: &Store, b: &Budget, name: &str) -> Option<FormKey> {
    store
        .winners()
        .filter(|w| !w.deleted && w.signature == *b"GLOB")
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .find(|k| {
            store
                .raw(k, &b.child())
                .ok()
                .flatten()
                .and_then(|r| bind::editor_id(&r))
                .is_some_and(|e| e.eq_ignore_ascii_case(name))
        })
}

/// The time scale a run has: the value of the last
/// `GlobalVariable.SetValue` of the global with a determined argument, or
/// the stored FLTV when no native of the run touched the global; `None`
/// when another native touched it after that or the argument is
/// undetermined (`vm_timer_time_scale_v1`).
pub(super) fn time_scale(vm: &Vm<'_>, store: &Store, g: &FormKey, _b: &Budget) -> Option<f64> {
    let key = bind::record_key(g);
    let touched: Vec<&Effect> = vm
        .effects
        .iter()
        .filter(|e| {
            e.receiver_object
                .and_then(|o| vm.world.objects[o].key.as_ref())
                .is_some_and(|k| *k == key)
                || e.args.iter().any(|a| a.contains(&key))
        })
        .collect();
    match touched.last() {
        None => crate::data::global_fltv(store, g, store.limits.max_memory)
            .ok()
            .map(f64::from),
        Some(e) if e.native == "globalvariable.setvalue" => {
            e.args.first().and_then(|a| a.parse::<f64>().ok())
        }
        Some(e) if e.native == "globalvariable.getvalue" => {
            // A read changes nothing: the value before it stands.
            let before: Vec<&&Effect> = touched
                .iter()
                .filter(|x| x.native != "globalvariable.getvalue")
                .collect();
            match before.last() {
                None => crate::data::global_fltv(store, g, store.limits.max_memory)
                    .ok()
                    .map(f64::from),
                Some(x) if x.native == "globalvariable.setvalue" => {
                    x.args.first().and_then(|a| a.parse::<f64>().ok())
                }
                _ => None,
            }
        }
        _ => None,
    }
}
