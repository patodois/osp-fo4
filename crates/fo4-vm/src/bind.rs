//! Binding scripts to game data (`spec/vm.md`, Binding): the `.pex` files
//! of a manifest's archives as a [`Library`], the scripts of a placed
//! reference instantiated from VMAD with their property values
//! (`vmad_reference_scripts_v1`, `vmad_property_sets_variable_v1`), and the
//! events a command line can deliver (`vm_event_handlers_v1`).
use crate::data::{key_record, record_object, DataHost};
use crate::{
    program::{Library, Script},
    rules,
    world::{InstId, ObjId, Origin, StructVal, Val},
    Outcome, Vm, VmError,
};
use fo4_core::{Budget, Result};
use fo4_pex::{index::lower, Pex};
use fo4_plugin::{
    typed::Store,
    vmad::{Property, PropertyValue, Vmad},
    FormKey, Record,
};
use fo4_vfs::Vfs;
use std::{fmt::Write as _, rc::Rc};

/// The `.pex` files of a manifest's archives, read when first named
/// (`scripts/<name with : as />.pex`, the winning entry).
pub struct VfsLibrary<'v> {
    vfs: &'v Vfs,
    /// Bound of one file read.
    pub max_memory: u64,
    /// Files read so far.
    pub files: u64,
    cache: std::collections::HashMap<String, Option<Rc<Script>>>,
}
impl<'v> VfsLibrary<'v> {
    pub fn new(vfs: &'v Vfs, max_memory: u64) -> Self {
        VfsLibrary {
            vfs,
            max_memory,
            files: 0,
            cache: Default::default(),
        }
    }
}
impl Library for VfsLibrary<'_> {
    fn load(&mut self, name: &str) -> std::result::Result<Option<Rc<Script>>, VmError> {
        if let Some(s) = self.cache.get(name) {
            return Ok(s.clone());
        }
        let s = self.read(name)?;
        self.cache.insert(name.to_string(), s.clone());
        Ok(s)
    }
}
impl VfsLibrary<'_> {
    fn read(&mut self, name: &str) -> std::result::Result<Option<Rc<Script>>, VmError> {
        if name.is_empty()
            || name.contains(['/', '\\'])
            || name.split(':').any(|p| p.is_empty() || p == "..")
        {
            return Ok(None);
        }
        let path = format!("scripts/{}.pex", name.replace(':', "/"));
        let missing = self.vfs.chain(&path).map(|c| c.is_empty()).unwrap_or(true);
        if missing {
            return Ok(None);
        }
        let b = Budget::new(self.max_memory);
        let bytes = self.vfs.read(&path, &b).map_err(|e| VmError::BadArgument {
            site: None,
            detail: format!("reading {path}: {e}"),
        })?;
        let p = Pex::read(&bytes, &path, &b).map_err(|e| VmError::BadArgument {
            site: None,
            detail: format!("reading {path}: {e}"),
        })?;
        self.files += 1;
        let s = Script::from_pex(&p, 0).filter(|s| s.name == name);
        Ok(s.map(Rc::new))
    }
}

/// An event a command line names, with the function it runs
/// (`vm_event_handlers_v1`).
pub const EVENTS: [(&str, &str); 9] = [
    ("activate", "onactivate"),
    ("trigger-enter", "ontriggerenter"),
    ("trigger-leave", "ontriggerleave"),
    ("load", "onload"),
    ("unload", "onunload"),
    ("init", "oninit"),
    ("reset", "onreset"),
    ("cell-attach", "oncellattach"),
    ("cell-detach", "oncelldetach"),
];

/// One event to deliver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A handler of [`EVENTS`] by its command-line name.
    Named(&'static str, &'static str),
    /// A TERM fragment by index.
    Fragment(u32),
}
impl Event {
    pub fn parse(s: &str) -> Option<Event> {
        if let Some(n) = s.strip_prefix("fragment:") {
            return n.parse().ok().map(Event::Fragment);
        }
        EVENTS
            .iter()
            .find(|e| e.0 == s)
            .map(|e| Event::Named(e.0, e.1))
    }
    pub fn name(&self) -> String {
        match self {
            Event::Named(n, _) => n.to_string(),
            Event::Fragment(i) => format!("fragment:{i}"),
        }
    }
}

/// A placed reference with its scripts instantiated.
#[derive(Debug, Clone)]
pub struct Bound {
    pub object: ObjId,
    /// Base record type.
    pub base_signature: [u8; 4],
    /// (script, instance), in VMAD order.
    pub instances: Vec<(String, InstId)>,
    /// The TERM fragment script's instance and its fragments
    /// (index, function), when the base has a fragment block.
    pub fragments: Option<(InstId, Vec<(u32, String)>)>,
    /// What the binding could not apply, as text.
    pub notes: Vec<String>,
}

/// Stable key of a record for object identity (`vm_object_identity_v1`).
pub fn record_key(k: &FormKey) -> String {
    format!("{}:{:08X}", k.plugin.to_ascii_lowercase(), k.object_id)
}

/// The record's EDID text, when it has one.
pub fn editor_id(r: &Record) -> Option<String> {
    let s = r.subrecords.iter().find(|s| s.signature == *b"EDID")?;
    let d = r.data(s);
    let d = d.strip_suffix(&[0]).unwrap_or(d);
    Some(String::from_utf8_lossy(d).into_owned())
}

/// A label for the effect log: record type, editor ID when there is one,
/// and the record key.
pub fn label(store: &Store, k: &FormKey, b: &Budget) -> Result<String> {
    let sig = store
        .signature(k)
        .map(|s| String::from_utf8_lossy(&s).into_owned())
        .unwrap_or_else(|| "????".into());
    let edid = store.raw(k, b)?.and_then(|r| editor_id(&r));
    Ok(match edid {
        Some(e) => format!("{sig} {e} {}", record_key(k)),
        None => format!("{sig} {}", record_key(k)),
    })
}

/// The base record of a placed reference (its NAME).
pub fn base_of(store: &Store, reference: &FormKey, b: &Budget) -> Result<Option<FormKey>> {
    let Some((r, plugin)) = store.raw_with_plugin(reference, b)? else {
        return Ok(None);
    };
    let Some(name) = r.subrecords.iter().find(|s| s.signature == *b"NAME") else {
        return Ok(None);
    };
    let d = r.data(name);
    if d.len() != 4 {
        return Ok(None);
    }
    let id = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
    store.order().remap(plugin, id, &r.source.file, name.offset)
}

/// The VMAD of a record with the plugin its FormIDs resolve through.
pub(crate) fn vmad_of(store: &Store, k: &FormKey, b: &Budget) -> Result<Option<(Record, usize)>> {
    let Some((r, plugin)) = store.raw_with_plugin(k, b)? else {
        return Ok(None);
    };
    if !r.subrecords.iter().any(|s| s.signature == *b"VMAD") {
        return Ok(None);
    }
    Ok(Some((r, plugin)))
}

pub(crate) struct Ctx<'s> {
    pub(crate) store: &'s Store,
    pub(crate) plugin: usize,
    pub(crate) file: String,
    pub(crate) b: &'s Budget,
}

/// A VMAD value as a run value of a declared type.
fn value(
    vm: &mut Vm<'_>,
    cx: &Ctx<'_>,
    v: &PropertyValue<'_>,
    ty: &str,
    notes: &mut Vec<String>,
) -> std::result::Result<Val, Box<dyn std::error::Error>> {
    Ok(match v {
        PropertyValue::None => Val::None,
        PropertyValue::Int(n) => {
            if ty == "float" {
                Val::Float(*n as f32)
            } else {
                Val::Int(*n)
            }
        }
        PropertyValue::Float(x) => Val::Float(*x),
        PropertyValue::Bool(x) => Val::Bool(*x),
        PropertyValue::String(t) => Val::Str(String::from_utf8_lossy(t).into_owned()),
        PropertyValue::Object(o) => {
            if o.form_id == 0 {
                return Ok(Val::None);
            }
            let Some(k) = cx.store.order().remap(cx.plugin, o.form_id, &cx.file, 0)? else {
                return Ok(vm.world.unknown(Origin::Rule(
                    rules::VMAD_VALUES,
                    format!("object value {:08X} names no plugin", o.form_id),
                )));
            };
            if cx.store.signature(&k).is_none() {
                return Ok(vm.world.unknown(Origin::Rule(
                    rules::VMAD_VALUES,
                    format!("object value {} names no winning record", record_key(&k)),
                )));
            }
            if o.alias >= 0 {
                // The alias of that index of the quest the FormID names
                // (`vmad_alias_value_v1`): one handle per quest and index.
                vm.rules.insert(rules::ALIAS_VALUE);
                let l = label(cx.store, &k, cx.b)?;
                let key = format!("{}#alias:{}", record_key(&k), o.alias);
                let o = vm
                    .world
                    .keyed_object(&format!("alias {} of {l}", o.alias), &key, ty);
                return Ok(Val::Object(o));
            }
            Val::Object(record_object(&mut vm.world, cx.store, &k, ty, cx.b)?)
        }
        PropertyValue::Struct(members) => make_struct(vm, cx, members, ty, notes)?,
        PropertyValue::Array(_, items) => {
            let elem = ty.strip_suffix("[]").unwrap_or("var").to_string();
            let mut out = Vec::new();
            for x in items {
                out.push(value(vm, cx, x, &elem, notes)?);
            }
            vm.world.array(&elem, out)
        }
        PropertyValue::StructArray(items) => {
            let elem = ty.strip_suffix("[]").unwrap_or("var").to_string();
            let mut out = Vec::new();
            for m in items {
                out.push(make_struct(vm, cx, m, &elem, notes)?);
            }
            vm.world.array(&elem, out)
        }
    })
}

fn make_struct(
    vm: &mut Vm<'_>,
    cx: &Ctx<'_>,
    members: &[Property<'_>],
    ty: &str,
    notes: &mut Vec<String>,
) -> std::result::Result<Val, Box<dyn std::error::Error>> {
    let Some((script, name)) = ty.split_once('#') else {
        notes.push(format!("struct value for a property of type {ty}"));
        return Ok(Val::None);
    };
    let s = vm.script(script)?;
    let Some(decl) = s.structs.iter().find(|d| d.name == name).cloned() else {
        notes.push(format!("struct {ty} not declared"));
        return Ok(Val::None);
    };
    let mut out = Vec::new();
    for (n, t, l) in &decl.members {
        let v = vm.initial(l, t);
        out.push((n.clone(), t.clone(), v));
    }
    for m in members {
        let n = lower(m.name);
        match out.iter().position(|x| x.0 == n) {
            Some(i) => {
                let t = out[i].1.clone();
                out[i].2 = value(vm, cx, &m.value, &t, notes)?;
            }
            None => notes.push(format!("struct {ty} has no member {n}")),
        }
    }
    vm.world.structs.push(StructVal {
        type_name: ty.to_string(),
        members: out,
    });
    Ok(Val::Struct(vm.world.structs.len() - 1))
}

/// Applies the properties of one VMAD script to an instance.
pub(crate) fn apply(
    vm: &mut Vm<'_>,
    cx: &Ctx<'_>,
    inst: InstId,
    script: &str,
    props: &[Property<'_>],
    notes: &mut Vec<String>,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    for p in props {
        let n = lower(p.name);
        let Some(ty) = vm.property_type(script, &n)? else {
            notes.push(format!(
                "{script}: property {n} is not declared; not applied"
            ));
            continue;
        };
        let v = value(vm, cx, &p.value, &ty, notes)?;
        match vm.set_property_variable(inst, &n, v) {
            Ok(true) => {}
            Ok(false) => notes.push(format!(
                "{script}: property {n} has no variable behind it; not applied"
            )),
            Err(e) => notes.push(format!("{script}: property {n}: {e}; not applied")),
        }
    }
    vm.rules.insert(rules::VMAD_VALUES);
    Ok(())
}

/// Applies the property values of one VMAD script entry, read from a
/// record of `plugin` stored in `file`, to an instance, exactly as binding
/// does (`vmad_property_sets_variable_v1`); what could not be applied goes
/// to `notes`. For census tools that check what the binder carries.
#[allow(clippy::too_many_arguments)]
pub fn apply_script(
    vm: &mut Vm<'_>,
    store: &Store,
    plugin: usize,
    file: &str,
    inst: InstId,
    script: &str,
    props: &[Property<'_>],
    notes: &mut Vec<String>,
    b: &Budget,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let cx = Ctx {
        store,
        plugin,
        file: file.to_string(),
        b,
    };
    apply(vm, &cx, inst, script, props, notes)
}

/// Instantiates the scripts of a placed reference from its base's VMAD and
/// its own (`vmad_reference_scripts_v1`), with their property values.
/// Returns `None` when the reference has no base.
pub fn bind_reference(
    vm: &mut Vm<'_>,
    store: &Store,
    reference: &FormKey,
    b: &Budget,
) -> std::result::Result<Option<Bound>, Box<dyn std::error::Error>> {
    let Some(base) = base_of(store, reference, b)? else {
        return Ok(None);
    };
    let object = record_object(&mut vm.world, store, reference, "objectreference", b)?;
    bind(vm, store, object, &base, Some(reference), b).map(Some)
}

/// Instantiates the scripts of a base record's VMAD on a new object
/// standing for one placement of it (the base census): no reference VMAD.
pub fn bind_base(
    vm: &mut Vm<'_>,
    store: &Store,
    base: &FormKey,
    b: &Budget,
) -> std::result::Result<Bound, Box<dyn std::error::Error>> {
    let l = format!("placement of {}", label(store, base, b)?);
    let object = vm.world.object(l, None, &["objectreference"]);
    vm.world.placement_bases.push((object, record_key(base)));
    bind(vm, store, object, base, None, b)
}

fn bind(
    vm: &mut Vm<'_>,
    store: &Store,
    object: ObjId,
    base: &FormKey,
    reference: Option<&FormKey>,
    b: &Budget,
) -> std::result::Result<Bound, Box<dyn std::error::Error>> {
    let base_signature = store.signature(base).unwrap_or(*b"????");
    vm.world.objects[object].attached = true;
    let mut bound = Bound {
        object,
        base_signature,
        instances: Vec::new(),
        fragments: None,
        notes: Vec::new(),
    };
    let records: Vec<(&FormKey, &str)> = std::iter::once((base, "base"))
        .chain(reference.map(|r| (r, "reference")))
        .collect();
    for (k, what) in records {
        let Some((r, plugin)) = vmad_of(store, k, b)? else {
            continue;
        };
        let cx = Ctx {
            store,
            plugin,
            file: r.source.file.clone(),
            b,
        };
        for s in r.subrecords.iter().filter(|s| s.signature == *b"VMAD") {
            let v = Vmad::read(r.data(s), r.signature, &r.source.file, s.offset, b)?;
            vm.rules.insert(rules::VMAD_SCRIPTS);
            for sc in &v.scripts {
                let name = lower(sc.name);
                let inst = match bound.instances.iter().find(|x| x.0 == name) {
                    Some(x) => x.1,
                    None => match vm.instantiate(Some(object), &name) {
                        Ok(i) => {
                            bound.instances.push((name.clone(), i));
                            i
                        }
                        Err(e) => {
                            bound
                                .notes
                                .push(format!("{what} script {name}: {e}; not attached"));
                            continue;
                        }
                    },
                };
                apply(vm, &cx, inst, &name, &sc.properties, &mut bound.notes)?;
            }
            if let Some(t) = &v.terminal {
                let name = lower(t.script.name);
                let form = record_object(&mut vm.world, store, k, "form", b)?;
                vm.world.objects[form].attached = true;
                match vm.instantiate(Some(form), &name) {
                    Ok(i) => {
                        apply(vm, &cx, i, &name, &t.script.properties, &mut bound.notes)?;
                        let list = t
                            .fragments
                            .iter()
                            .map(|f| (f.index, lower(f.function)))
                            .collect();
                        bound.fragments = Some((i, list));
                    }
                    Err(e) => bound
                        .notes
                        .push(format!("fragment script {name}: {e}; not attached")),
                }
            }
        }
    }
    Ok(bound)
}

/// The arguments an event function receives: none, or the player handle
/// for one object parameter (`vm_event_handlers_v1`). `None` when the
/// function takes other parameters.
pub(crate) fn event_args(vm: &mut Vm<'_>, params: &[(String, String)]) -> Option<Vec<Val>> {
    match params {
        [] => Some(Vec::new()),
        [(_, t)]
            if !matches!(t.as_str(), "int" | "float" | "bool" | "string") && !t.ends_with("[]") =>
        {
            let ty = vm
                .try_script("game")
                .ok()
                .flatten()
                .and_then(|g| g.function("", "getplayer").map(|f| f.return_type.clone()))
                .unwrap_or_default();
            let p = vm.world.player(&ty);
            Some(vec![Val::Object(p)])
        }
        _ => None,
    }
}

/// Delivers one event to every script instance of a bound reference, in
/// VMAD order; one outcome per instance that has a function for it (a
/// TERM fragment: one outcome).
pub fn deliver(vm: &mut Vm<'_>, bound: &Bound, event: &Event) -> Vec<(String, Outcome)> {
    vm.rules.insert(rules::EVENTS);
    let mut out = Vec::new();
    match event {
        Event::Named(_, f) => {
            for (script, inst) in &bound.instances {
                let func = match vm.find_handler(*inst, f) {
                    Ok(Some(func)) => func,
                    Ok(None) => continue,
                    Err(e) => {
                        out.push((script.clone(), Outcome::Failed(e)));
                        continue;
                    }
                };
                let Some(args) = event_args(vm, &func.params) else {
                    out.push((
                        script.clone(),
                        Outcome::Failed(VmError::BadArgument {
                            site: None,
                            detail: format!("event {f} takes parameters the event does not give"),
                        }),
                    ));
                    continue;
                };
                let o = vm.deliver(*inst, f, args);
                out.push((script.clone(), o));
            }
            // The instances registered for this event of the reference
            // (`vm_remote_events_v1`), after its own scripts.
            let mut args = |vm: &mut Vm<'_>, p: &[(String, String)]| event_args(vm, p);
            for (script, o) in crate::events::deliver_remote(vm, bound.object, f, &mut args, false)
            {
                out.push((format!("{script} (remote)"), o));
            }
        }
        Event::Fragment(i) => {
            let Some((inst, list)) = &bound.fragments else {
                return out;
            };
            let Some((_, fname)) = list.iter().find(|x| x.0 == *i) else {
                return out;
            };
            let script = vm.world.instances[*inst].script.clone();
            let me = Val::Object(bound.object);
            let o = vm.deliver(*inst, fname, vec![me]);
            out.push((script, o));
        }
    }
    out
}

impl Vm<'_> {
    /// The function an event runs on an instance, `None` when only an
    /// empty function of a script without variables (a root declaration)
    /// would run: such a script's event function has no instructions.
    pub fn find_handler(
        &mut self,
        inst: InstId,
        name: &str,
    ) -> std::result::Result<Option<Rc<crate::Func>>, VmError> {
        match self.dispatch(inst, name) {
            Ok(Some(f)) if f.code.is_empty() => Ok(None),
            Ok(f) => Ok(f),
            Err(crate::Stop::Error(e)) => Err(*e),
            Err(crate::Stop::Blocked(b)) => Err(VmError::BadArgument {
                site: None,
                detail: format!("dispatch blocked on {}", b.on.text()),
            }),
        }
    }
}

/// The placed references of a cell as `script_run` and the viewer's
/// `--script-event` number them: the active ones, then the initially
/// disabled ones, each in stored order.
pub fn placed(store: &Store, cell: &FormKey, b: &Budget) -> Result<Vec<FormKey>> {
    let ch = store.children(cell, b)?;
    Ok(ch
        .active
        .iter()
        .chain(&ch.disabled)
        .map(|c| c.key.clone())
        .collect())
}

/// One run: a reference bound, events delivered in order (and queued
/// timers fired when asked), with everything the run produced.
pub struct Run {
    pub bound: Bound,
    /// (event, script, outcome) in delivery order; timers as `timer:<id>`.
    pub outcomes: Vec<(String, String, Outcome)>,
    pub effects: Vec<crate::Effect>,
    pub rules: std::collections::BTreeSet<&'static str>,
    pub world: crate::World,
    /// The data host's answers by native and kind ([`Mode::Data`]).
    pub answers: std::collections::BTreeMap<(String, crate::data::Answer), u64>,
    /// Handles given scripts when first needed, by kind.
    pub attached: std::collections::BTreeMap<&'static str, u64>,
    /// What attaching could not apply.
    pub attach_notes: Vec<String>,
    /// [`Mode::Quest`]: the quest jobs run inside the events, in order of
    /// starting, and the jobs left queued.
    pub quest_jobs: Vec<crate::quest::JobRun>,
    pub quest_jobs_left: usize,
    /// Registrations and deliveries of remote and custom events, and story
    /// events offered, by text.
    pub event_counts: std::collections::BTreeMap<String, u64>,
}

/// Which host a run uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The recording host alone, no record's scripts attached on need, as
    /// before the data host (`script_run --recording`).
    Recording,
    /// The data host ([`DataHost`]) and the scripts of quests, aliases and
    /// placed references attached when first needed ([`RecordScripts`]).
    Data,
    /// As [`Mode::Data`], with the quest host
    /// ([`crate::quest::QuestHost`]): quest natives answered from the
    /// quest state of the run (isolated scenario,
    /// `quest_state_isolated_v1`), stage fragments, quest events and
    /// OnStageSet run inside the native call that brought them
    /// ([`crate::quest::Runner`], at most [`QUEST_JOBS`] per run).
    Quest,
}

/// Quest jobs a run in [`Mode::Quest`] runs at most.
pub const QUEST_JOBS: usize = 256;

/// Attaches the scripts of the record behind a handle when a run first
/// needs an instance on it (`vm_record_scripts_attach_v1`): a quest's VMAD
/// scripts on the quest's handle, the scripts of a quest's alias entry on
/// the alias handle (`vmad_alias_value_v1`), and a placed reference's
/// (REFR or ACHR) scripts as [`bind_reference`] attaches them, each with
/// its VMAD property values. Other records get nothing.
pub struct RecordScripts<'s> {
    pub store: &'s Store,
    pub max_memory: u64,
    /// Handles given scripts, by kind (`quest`, `alias`, `reference`).
    pub attached: std::collections::BTreeMap<&'static str, u64>,
    /// What could not be attached or applied, as text.
    pub notes: Vec<String>,
}
impl<'s> RecordScripts<'s> {
    pub fn new(store: &'s Store, max_memory: u64) -> Self {
        RecordScripts {
            store,
            max_memory,
            attached: Default::default(),
            notes: Vec::new(),
        }
    }
    fn scripts(
        &mut self,
        vm: &mut Vm<'_>,
        o: ObjId,
        cx: &Ctx<'_>,
        list: &[fo4_plugin::vmad::Script<'_>],
    ) -> std::result::Result<bool, Box<dyn std::error::Error>> {
        let mut any = false;
        for sc in list {
            let name = lower(sc.name);
            match vm.instantiate(Some(o), &name) {
                Ok(i) => {
                    any = true;
                    apply(vm, cx, i, &name, &sc.properties, &mut self.notes)?;
                }
                Err(e) => self
                    .notes
                    .push(format!("attached script {name}: {e}; not attached")),
            }
        }
        Ok(any)
    }
    fn quest(
        &mut self,
        vm: &mut Vm<'_>,
        o: ObjId,
        quest: &FormKey,
        alias: Option<i16>,
        b: &Budget,
    ) -> std::result::Result<bool, Box<dyn std::error::Error>> {
        let Some((r, plugin)) = vmad_of(self.store, quest, b)? else {
            return Ok(false);
        };
        let cx = Ctx {
            store: self.store,
            plugin,
            file: r.source.file.clone(),
            b,
        };
        let mut any = false;
        for s in r.subrecords.iter().filter(|s| s.signature == *b"VMAD") {
            let v = Vmad::read(r.data(s), r.signature, &r.source.file, s.offset, b)?;
            match alias {
                None => any |= self.scripts(vm, o, &cx, &v.scripts)?,
                Some(i) => {
                    let entries = v.quest.iter().flat_map(|q| &q.aliases);
                    for a in entries.filter(|a| a.object.alias == i) {
                        any |= self.scripts(vm, o, &cx, &a.scripts)?;
                    }
                }
            }
        }
        Ok(any)
    }
    fn attach_key(
        &mut self,
        vm: &mut Vm<'_>,
        o: ObjId,
        key: &str,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let b = Budget::new(self.max_memory);
        if let Some((q, i)) = key.split_once("#alias:") {
            let (Some(q), Ok(i)) = (key_record(q), i.parse::<i16>()) else {
                return Ok(());
            };
            if self.quest(vm, o, &q, Some(i), &b)? {
                *self.attached.entry("alias").or_default() += 1;
                vm.rules.insert(rules::ATTACH);
            }
            return Ok(());
        }
        let Some(k) = key_record(key) else {
            return Ok(());
        };
        match self.store.signature(&k).as_ref().map(|s| &s[..]) {
            Some(b"QUST") => {
                if self.quest(vm, o, &k, None, &b)? {
                    *self.attached.entry("quest").or_default() += 1;
                    vm.rules.insert(rules::ATTACH);
                }
            }
            Some(b"REFR" | b"ACHR") => {
                let Some(base) = base_of(self.store, &k, &b)? else {
                    return Ok(());
                };
                let bound = bind(vm, self.store, o, &base, Some(&k), &b)?;
                self.notes.extend(bound.notes);
                if !bound.instances.is_empty() {
                    *self.attached.entry("reference").or_default() += 1;
                    vm.rules.insert(rules::ATTACH);
                }
            }
            _ => {}
        }
        Ok(())
    }
}
impl crate::Attach for RecordScripts<'_> {
    fn attach(&mut self, vm: &mut Vm<'_>, o: ObjId) -> std::result::Result<(), VmError> {
        let Some(key) = vm.world.objects[o].key.clone() else {
            return Ok(());
        };
        self.attach_key(vm, o, &key)
            .map_err(|e| VmError::BadArgument {
                site: None,
                detail: format!("attaching the scripts of {key}: {e}"),
            })
    }
}

/// Binds a placed reference (or, with `reference` `None`, a base alone)
/// in a fresh interpreter with the data host ([`Mode::Data`]), delivers
/// the events, and fires up to `timers` queued timers.
pub fn run(
    lib: &mut dyn Library,
    store: &Store,
    reference: Option<&FormKey>,
    base: &FormKey,
    events: &[Event],
    timers: usize,
    b: &Budget,
) -> std::result::Result<Run, Box<dyn std::error::Error>> {
    run_in(Mode::Data, lib, store, reference, base, events, timers, b)
}

/// [`run`] with a chosen host, under [`Options::of`] that host.
#[allow(clippy::too_many_arguments)]
pub fn run_in(
    mode: Mode,
    lib: &mut dyn Library,
    store: &Store,
    reference: Option<&FormKey>,
    base: &FormKey,
    events: &[Event],
    timers: usize,
    b: &Budget,
) -> std::result::Result<Run, Box<dyn std::error::Error>> {
    let o = Options::of(mode);
    run_with(mode, &o, lib, store, reference, base, events, timers, b)
}

/// Switches of a run beyond its host, each reproducing an earlier reading
/// when turned off.
#[derive(Debug, Clone)]
pub struct Options {
    /// Deliver remote events the run raises ([`Mode::Quest`] only;
    /// `vm_remote_events_v1`).
    pub remote: bool,
    /// Deliver custom events, and when ([`Mode::Quest`] only;
    /// `vm_custom_events_v1`, `vm_custom_event_order_v1`).
    pub custom: Option<crate::events::CustomOrder>,
    /// Offer the script event to these story manager nodes
    /// ([`Mode::Quest`] only; `sm_script_event_v1`).
    pub story: Option<std::rc::Rc<fo4_plugin::story::Tree>>,
    /// `IsDisabled` follows enable parent chains
    /// (`vm_host_is_disabled_chain_v1`; data and quest hosts).
    pub enable_chain: bool,
    /// The condition evaluators added after the first five
    /// ([`crate::cond::Switches`]; quest host only). A quest variable is
    /// read from the run's attached instances only: the run's library is
    /// the interpreter's, so a quest the run did not attach leaves it
    /// undetermined.
    pub cond: crate::cond::Switches,
}
impl Options {
    /// The candidate readings for a host: events delivered with the quest
    /// host, the enable chain followed with the data and quest hosts, no
    /// story manager nodes (a caller reads them once and passes them).
    pub fn of(mode: Mode) -> Options {
        Options {
            remote: mode == Mode::Quest,
            custom: (mode == Mode::Quest).then_some(crate::events::CustomOrder::Queued),
            story: None,
            enable_chain: mode != Mode::Recording,
            cond: crate::cond::Switches::all(),
        }
    }
}

/// [`run_in`] with chosen [`Options`].
#[allow(clippy::too_many_arguments)]
pub fn run_with(
    mode: Mode,
    opts: &Options,
    lib: &mut dyn Library,
    store: &Store,
    reference: Option<&FormKey>,
    base: &FormKey,
    events: &[Event],
    timers: usize,
    b: &Budget,
) -> std::result::Result<Run, Box<dyn std::error::Error>> {
    let max = store.limits.max_memory;
    let mut recording = crate::RecordingHost::default();
    let mut data = DataHost::new(store, max);
    data.enable_chain = opts.enable_chain;
    let mut quest = crate::quest::QuestHost::new(store, max);
    quest.data.enable_chain = opts.enable_chain;
    quest.story = opts.story.clone();
    quest.cond.switches = opts.cond.clone();
    let mut attach = RecordScripts::new(store, max);
    let mut runner = crate::quest::Runner::new(store, max, QUEST_JOBS);
    let host: &mut dyn crate::Host = match mode {
        Mode::Recording => &mut recording,
        Mode::Data => &mut data,
        Mode::Quest => &mut quest,
    };
    let mut vm = Vm::new(lib, host);
    if mode != Mode::Recording {
        vm.attach = Some(&mut attach);
    }
    if mode == Mode::Quest {
        vm.nested = Some(&mut runner);
        vm.world.events.remote = opts.remote;
        vm.world.events.custom = opts.custom;
    }
    let bound = match reference {
        Some(r) => bind_reference(&mut vm, store, r, b)?.ok_or("reference without a base")?,
        None => bind_base(&mut vm, store, base, b)?,
    };
    let mut outcomes = Vec::new();
    for e in events {
        for (script, o) in deliver(&mut vm, &bound, e) {
            outcomes.push((e.name(), script, o));
        }
        // Custom events sent during the delivery (queued reading), and
        // what they bring.
        if mode == Mode::Quest && !vm.world.events.later.is_empty() {
            let top = crate::quest::run_jobs(&mut vm, store, QUEST_JOBS, b);
            vm.world.quests.nested_runs.extend(top);
        }
    }
    for (inst, id, o) in vm.fire_timers(timers) {
        let script = vm.world.instances[inst].script.clone();
        outcomes.push((format!("timer:{}", vm.world.render(&id)), script, o));
    }
    let events_counts = std::mem::take(&mut vm.world.events.counts);
    let mut quest_jobs = std::mem::take(&mut vm.world.quests.nested_runs);
    quest_jobs.sort_by_key(|r| r.seq);
    let quest_jobs_left = vm.world.quests.queue.len();
    let effects = std::mem::take(&mut vm.effects);
    let rules = std::mem::take(&mut vm.rules);
    let world = std::mem::take(&mut vm.world);
    drop(vm);
    let mut answers = std::mem::take(&mut data.answers);
    for (k, v) in quest.answers.iter().chain(quest.data.answers.iter()) {
        *answers.entry(k.clone()).or_default() += v;
    }
    Ok(Run {
        bound,
        outcomes,
        effects,
        rules,
        world,
        answers,
        attached: attach.attached,
        attach_notes: attach.notes,
        quest_jobs,
        quest_jobs_left,
        event_counts: {
            let mut c = events_counts;
            for (k, v) in &quest.story_counts {
                *c.entry(k.clone()).or_default() += v;
            }
            c
        },
    })
}

/// The visibility changes a run's effect log names on keyed objects
/// (`vm_disable_hides_reference_v1`): (record key, shown), in log order.
pub fn visibility_changes(r: &Run) -> Vec<(String, bool)> {
    r.effects
        .iter()
        .filter_map(|e| {
            let shown = match e.native.as_str() {
                "objectreference.disable" => false,
                "objectreference.enable" => true,
                _ => return None,
            };
            let key = r.world.objects.get(e.receiver_object?)?.key.clone()?;
            Some((key, shown))
        })
        .collect()
}

/// One line for an outcome.
pub fn outcome_text(o: &Outcome) -> String {
    match o {
        Outcome::Completed(v) => format!("completed ({v:?})"),
        Outcome::Blocked(b) => format!(
            "blocked on {} at {}",
            b.on.text(),
            b.at.as_ref().map(|s| s.to_string()).unwrap_or_default()
        ),
        Outcome::Failed(e) => format!("failed: {e}"),
    }
}

/// The effect log of a run as text: the object, its scripts and their
/// states, binding notes, each event's outcome, every native call, the
/// visibility changes and the rules relied on.
pub fn effect_log(r: &Run) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "object {}  base type {}",
        r.world.objects[r.bound.object].label,
        String::from_utf8_lossy(&r.bound.base_signature)
    );
    for (n, i) in &r.bound.instances {
        let _ = writeln!(s, "script {n} (state {:?})", r.world.instances[*i].state);
    }
    if let Some((_, f)) = &r.bound.fragments {
        let _ = writeln!(s, "terminal fragments {}", f.len());
    }
    for n in &r.bound.notes {
        let _ = writeln!(s, "note: {n}");
    }
    for (e, script, o) in &r.outcomes {
        let _ = writeln!(s, "event {e} -> {script}: {}", outcome_text(o));
    }
    let _ = writeln!(s, "effects ({}):", r.effects.len());
    for e in &r.effects {
        let _ = writeln!(
            s,
            "  {}{}({}{}) = {}    [from {}]",
            "  ".repeat(e.depth),
            e.native,
            e.receiver
                .as_ref()
                .map(|x| format!("on {x}; "))
                .unwrap_or_default(),
            e.args.join(", "),
            e.result,
            e.caller,
        );
    }
    for (k, shown) in visibility_changes(r) {
        let _ = writeln!(
            s,
            "visibility: {k} {}",
            if shown { "shown" } else { "hidden" }
        );
    }
    let rules: Vec<&str> = r.rules.iter().copied().collect();
    let _ = writeln!(s, "rules (pending): {}", rules.join(" "));
    s
}
