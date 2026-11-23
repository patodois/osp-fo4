//! The data host (`spec/vm.md`, Data host): the recording host, plus the
//! natives whose result is a fact the installed plugins store, answered by
//! a lookup over the records (each a pending rule in `rules`). Facts are
//! read as the data stores them, as at the start of a game before any
//! script ran (`vm_host_data_state_v1`); a native of the same run that can
//! change a fact makes it undetermined, except `Enable` and `Disable`,
//! whose change `IsDisabled` answers. Every other native is answered as the
//! recording host answers it.
use crate::{
    bind::{label, record_key},
    host::{Host, NativeCall, RecordingHost, Reply},
    rules,
    world::{ObjId, Val, World},
};
use fo4_core::Budget;
use fo4_plugin::{typed::Store, FormKey, Record};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

/// The engine script type of a record handle by record type
/// (`vm_record_signature_type_v1`): recognised from prior knowledge and
/// checked by the census of VMAD object values (`host_data_census`), which
/// finds the declared type of the property holding a record of a listed
/// type compatible with it (the type, a parent of it, or a script
/// extending it) in all but 19 of the values it checks (`spec/vm.md`, Data
/// host).
pub const SIGNATURE_TYPES: [([u8; 4], &str); 37] = [
    (*b"ACHR", "actor"),
    (*b"REFR", "objectreference"),
    (*b"ACTI", "activator"),
    (*b"ALCH", "potion"),
    (*b"AMMO", "ammo"),
    (*b"ARMO", "armor"),
    (*b"AVIF", "actorvalue"),
    (*b"BOOK", "book"),
    (*b"CELL", "cell"),
    (*b"CMPO", "component"),
    (*b"CONT", "container"),
    (*b"DOOR", "door"),
    (*b"ENCH", "enchantment"),
    (*b"EXPL", "explosion"),
    (*b"FACT", "faction"),
    (*b"FLST", "formlist"),
    (*b"FURN", "furniture"),
    (*b"GLOB", "globalvariable"),
    (*b"HAZD", "hazard"),
    (*b"IDLM", "idlemarker"),
    (*b"IMAD", "imagespacemodifier"),
    (*b"KEYM", "key"),
    (*b"KYWD", "keyword"),
    (*b"LCTN", "location"),
    (*b"LIGH", "light"),
    (*b"LVLI", "leveleditem"),
    (*b"MESG", "message"),
    (*b"MGEF", "magiceffect"),
    (*b"MISC", "miscobject"),
    (*b"NPC_", "actorbase"),
    (*b"PERK", "perk"),
    (*b"QUST", "quest"),
    (*b"SCEN", "scene"),
    (*b"SPEL", "spell"),
    (*b"STAT", "static"),
    (*b"TERM", "terminal"),
    (*b"WEAP", "weapon"),
];

/// The engine type of a record type, when [`SIGNATURE_TYPES`] lists it.
pub fn signature_type(sig: [u8; 4]) -> Option<&'static str> {
    SIGNATURE_TYPES
        .iter()
        .find(|(s, _)| *s == sig)
        .map(|(_, t)| *t)
}

/// The record behind a key made by [`record_key`] (`plugin:XXXXXXXX`);
/// `None` for other keys (an alias key carries `#alias:`).
pub fn key_record(key: &str) -> Option<FormKey> {
    let (plugin, id) = key.rsplit_once(':')?;
    if id.len() != 8 || plugin.contains('#') {
        return None;
    }
    Some(FormKey {
        plugin: plugin.to_string(),
        object_id: u32::from_str_radix(id, 16).ok()?,
    })
}

/// The handle of a winning record, created when absent, known to be of
/// `ty` and of its record type's engine type (`vm_record_signature_type_v1`).
pub fn record_object(
    w: &mut World,
    store: &Store,
    k: &FormKey,
    ty: &str,
    b: &Budget,
) -> fo4_core::Result<ObjId> {
    let l = label(store, k, b)?;
    let o = w.keyed_object(&l, &record_key(k), ty);
    if let Some(t) = store.signature(k).and_then(signature_type) {
        w.objects[o].signature_type = Some(t.to_string());
    }
    Ok(o)
}

/// The first u32 of each 4-byte-aligned word pair of a record's XLKR
/// subrecords, resolved through the record's plugin: (keyword, reference)
/// (`vm_host_linked_ref_xlkr_v1`). `None` when a subrecord is not a
/// multiple of 8 bytes or a FormID does not resolve.
pub fn linked_refs(
    store: &Store,
    r: &Record,
    plugin: usize,
) -> Option<Vec<(Option<FormKey>, Option<FormKey>)>> {
    let mut out = Vec::new();
    for s in r.subrecords.iter().filter(|s| s.signature == *b"XLKR") {
        let d = r.data(s);
        let (pairs, rest) = d.as_chunks::<8>();
        if !rest.is_empty() {
            return None;
        }
        for c in pairs {
            let kw = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
            let to = u32::from_le_bytes([c[4], c[5], c[6], c[7]]);
            let o = store.order();
            let kw = o.remap(plugin, kw, &r.source.file, s.offset).ok()?;
            let to = o.remap(plugin, to, &r.source.file, s.offset).ok()?;
            out.push((kw, to));
        }
    }
    Some(out)
}

/// The f32 of a GLOB's one FLTV (`vm_host_global_fltv_v1`).
pub fn global_fltv(store: &Store, k: &FormKey, max_memory: u64) -> Result<f32, &'static str> {
    let b = Budget::new(max_memory);
    let (r, _) = store
        .raw_with_plugin(k, &b)
        .ok()
        .flatten()
        .ok_or("the global names no winning record")?;
    let mut v = r.subrecords.iter().filter(|s| s.signature == *b"FLTV");
    let (Some(s), None) = (v.next(), v.next()) else {
        return Err("the global has not one FLTV");
    };
    let d: [u8; 4] = r.data(s).try_into().map_err(|_| "FLTV is not 4 bytes")?;
    Ok(f32::from_le_bytes(d))
}

/// Whether the first record of an enable parent chain is enabled under the
/// rules of `spec/enable-parent.md`, as `fo4_scene::enable` decides it:
/// the root starts as its header flag 0x800 says, each record below it as
/// its parent, inverted by flag bit 0 (`xesp_child_follows_parent_v1`,
/// `xesp_initial_state_chain_v1`, `xesp_flag_bit0_opposite_v1`; bit 1 does
/// not change presence, `xesp_flag_bit1_not_presence_v1`), a record this
/// run enabled or disabled itself taking that state
/// (`xesp_script_change_propagates_v1`). A chain those rules do not decide
/// (another flag bit, an end other than a record without XESP, a record
/// below the root with its own flag 0x800) is an `Err` with its reason.
/// Returns the state and the rules relied on.
pub fn chain_state(
    ch: &fo4_plugin::enable::Chain,
    set: &HashMap<FormKey, bool>,
) -> Result<(bool, Vec<&'static str>), &'static str> {
    chain_state_with(ch, set, false)
}

/// [`chain_state`], with `ignore_child_flags` reading a flag 0x800 below
/// the root as carrying no state of its own
/// (`xesp_child_own_flag_ignored_v1`).
pub fn chain_state_with(
    ch: &fo4_plugin::enable::Chain,
    set: &HashMap<FormKey, bool>,
    ignore_child_flags: bool,
) -> Result<(bool, Vec<&'static str>), &'static str> {
    use fo4_plugin::enable::ChainEnd;
    let mut used = vec![rules::XESP_FOLLOWS, rules::XESP_INITIAL];
    let bits = ch
        .nodes
        .iter()
        .filter_map(|n| n.link.as_ref())
        .fold(0u8, |a, l| a | l.flags);
    if bits & !3 != 0 {
        return Err("the chain carries an enable parent flag bit without a rule");
    }
    if bits & 1 != 0 {
        used.push(rules::XESP_OPPOSITE);
    }
    if bits & 2 != 0 {
        used.push(rules::XESP_BIT1);
    }
    match ch.end {
        ChainEnd::Root => {}
        ChainEnd::Cycle(_) => return Err("the enable parent chain loops"),
        ChainEnd::TooDeep => return Err("the enable parent chain is too deep"),
        _ => return Err("the enable parent chain names no record"),
    }
    let n = ch.nodes.len();
    if ch
        .nodes
        .iter()
        .take(n.saturating_sub(1))
        .any(|x| x.initially_disabled)
    {
        if !ignore_child_flags {
            return Err(
                "a record of the chain below its root carries its own initially-disabled flag",
            );
        }
        used.push(rules::XESP_CHILD_FLAG_IGNORED);
    }
    let mut nodes = ch.nodes.iter().rev();
    let root = nodes.next().ok_or("an empty chain")?;
    let mut touched = false;
    let mut on = match set.get(&root.key) {
        Some(v) => {
            touched = true;
            *v
        }
        None => !root.initially_disabled,
    };
    for x in nodes {
        let opposite = x.link.as_ref().is_some_and(|l| l.flags & 1 != 0);
        on = match set.get(&x.key) {
            Some(v) => {
                touched = true;
                *v
            }
            None => on != opposite,
        };
    }
    if touched {
        used.push(rules::XESP_PROPAGATES);
    }
    Ok((on, used))
}

/// One answer of the data host, counted by `script_run --census`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Answer {
    /// A record handle or an array of them.
    Record,
    /// None, or an empty array.
    NoRecord,
    /// A number or a truth value read from the data.
    Value,
    /// The data does not answer: the reason.
    Open(&'static str),
}

/// The data host.
pub struct DataHost<'s> {
    pub store: &'s Store,
    pub max_memory: u64,
    pub recording: RecordingHost,
    used: Vec<&'static str>,
    /// `Enable` (false) and `Disable` (true) of this run, last per object.
    disabled: HashMap<ObjId, bool>,
    /// Facts a native of this run may have changed: (object, fact).
    changed: HashSet<(ObjId, &'static str)>,
    /// Answers by `native` and kind.
    pub answers: BTreeMap<(String, Answer), u64>,
    /// `IsDisabled` of a reference with an enable parent follows its chain
    /// (`vm_host_is_disabled_chain_v1`); `false` reads the header flag
    /// alone, as before.
    pub enable_chain: bool,
    /// A record below its chain's root that carries its own
    /// initially-disabled flag takes its parent's state as the others do
    /// (`xesp_child_own_flag_ignored_v1`; `quest_run
    /// --enable-child-flag-ignored`). Off by default: such a chain is
    /// undetermined, as before.
    pub child_flag_ignored: bool,
    /// Globals' stored values read so far ([`DataHost::global_stored`]):
    /// the records do not change during a run.
    globals: RefCell<HashMap<FormKey, Result<f32, &'static str>>>,
    /// References' bases read so far ([`DataHost::base_stored`]).
    bases: RefCell<HashMap<FormKey, Result<FormKey, &'static str>>>,
}

/// Natives of the data host, lower case `script.function`, with the rule
/// each relies on.
pub const NATIVES: [(&str, &str); 10] = [
    ("objectreference.getlinkedref", rules::LINKED_REF),
    ("objectreference.getlinkedrefchain", rules::LINKED_REF_CHAIN),
    (
        "objectreference.countlinkedrefchain",
        rules::LINKED_REF_CHAIN,
    ),
    ("objectreference.isdisabled", rules::IS_DISABLED),
    ("objectreference.getbaseobject", rules::BASE_OBJECT),
    ("game.getformfromfile", rules::FORM_FROM_FILE),
    ("globalvariable.getvalue", rules::GLOBAL_VALUE),
    ("formlist.hasform", rules::FORM_LIST),
    ("formlist.getsize", rules::FORM_LIST),
    ("formlist.getat", rules::FORM_LIST),
];

/// Natives that read a global or a form list without changing it.
const PURE_READS: [&str; 6] = [
    "globalvariable.getvalue",
    "formlist.hasform",
    "formlist.getsize",
    "formlist.getat",
    "formlist.find",
    "form.haskeyword",
];

type Got = Result<(Val, Answer), &'static str>;

impl<'s> DataHost<'s> {
    pub fn new(store: &'s Store, max_memory: u64) -> Self {
        DataHost {
            store,
            max_memory,
            recording: RecordingHost::default(),
            used: Vec::new(),
            disabled: HashMap::new(),
            changed: HashSet::new(),
            answers: BTreeMap::new(),
            enable_chain: true,
            child_flag_ignored: false,
            globals: RefCell::new(HashMap::new()),
            bases: RefCell::new(HashMap::new()),
        }
    }
    /// A global's stored value ([`global_fltv`]), read once per run.
    pub fn global_stored(&self, k: &FormKey) -> Result<f32, &'static str> {
        if let Some(x) = self.globals.borrow().get(k) {
            return *x;
        }
        let x = global_fltv(self.store, k, self.max_memory);
        self.globals.borrow_mut().insert(k.clone(), x);
        x
    }
    /// A placed reference's base (`NAME`, `crate::bind::base_of`), read
    /// once per run; as the condition evaluator names its failures.
    pub fn base_stored(&self, reference: &FormKey) -> Result<FormKey, &'static str> {
        if let Some(x) = self.bases.borrow().get(reference) {
            return x.clone();
        }
        let b = Budget::new(self.max_memory);
        let x = crate::bind::base_of(self.store, reference, &b)
            .map_err(|_| "the reference could not be read")
            .and_then(|o| o.ok_or("the reference has no base"));
        self.bases.borrow_mut().insert(reference.clone(), x.clone());
        x
    }
    fn using(&mut self, r: &'static str) {
        if !self.used.contains(&r) {
            self.used.push(r);
        }
    }
    /// The record of an object handle.
    fn record_of(w: &World, v: Option<&Val>) -> Option<(ObjId, Option<FormKey>)> {
        match v? {
            Val::Object(o) => Some((*o, w.objects[*o].key.as_deref().and_then(key_record))),
            _ => None,
        }
    }
    fn raw(&self, k: &FormKey) -> Option<(Record, usize)> {
        let b = Budget::new(self.max_memory);
        self.store.raw_with_plugin(k, &b).ok().flatten()
    }
    fn object(&self, w: &mut World, k: &FormKey, ty: &str) -> Result<Val, &'static str> {
        let b = Budget::new(self.max_memory);
        record_object(w, self.store, k, ty, &b)
            .map(Val::Object)
            .map_err(|_| "a record could not be read")
    }
    /// The keyword a call names: `Ok(None)` for none.
    fn keyword(w: &World, v: Option<&Val>) -> Result<Option<FormKey>, &'static str> {
        match v {
            None | Some(Val::None) => Ok(None),
            Some(Val::Object(o)) => w.objects[*o]
                .key
                .as_deref()
                .and_then(key_record)
                .map(Some)
                .ok_or("the keyword is not a record handle"),
            _ => Err("the keyword is undetermined"),
        }
    }
    /// The reference linked from `k` by `kw` (`vm_host_linked_ref_xlkr_v1`).
    fn linked(&self, k: &FormKey, kw: &Option<FormKey>) -> Result<Option<FormKey>, &'static str> {
        let (r, plugin) = self.raw(k).ok_or("the reference names no winning record")?;
        let links = linked_refs(self.store, &r, plugin).ok_or("an XLKR is not word pairs")?;
        let mut hits = links.iter().filter(|(x, _)| x == kw);
        let first = hits.next();
        if hits.next().is_some() {
            return Err("two linked references with one keyword");
        }
        match first {
            None => Ok(None),
            Some((_, None)) => Ok(None),
            Some((_, Some(to))) => {
                if self.store.signature(to).is_none() {
                    return Err("the linked reference names no winning record");
                }
                Ok(Some(to.clone()))
            }
        }
    }
    fn get_linked_ref(&mut self, c: &NativeCall, w: &mut World) -> Got {
        let (o, k) =
            Self::record_of(w, c.receiver.as_ref()).ok_or("the receiver is not an object")?;
        if self.changed.contains(&(o, "linked")) {
            return Err("changed by this run");
        }
        let k = k.ok_or("the receiver is not a record")?;
        let kw = Self::keyword(w, c.args.first())?;
        match self.linked(&k, &kw)? {
            None => Ok((Val::None, Answer::NoRecord)),
            Some(to) => Ok((self.object(w, &to, "objectreference")?, Answer::Record)),
        }
    }
    fn linked_chain(&mut self, c: &NativeCall, w: &mut World, count: bool) -> Got {
        let (o, k) =
            Self::record_of(w, c.receiver.as_ref()).ok_or("the receiver is not an object")?;
        if self.changed.contains(&(o, "linked")) {
            return Err("changed by this run");
        }
        let mut at = k.ok_or("the receiver is not a record")?;
        let kw = Self::keyword(w, c.args.first())?;
        let max = match c.args.get(1) {
            Some(Val::Int(n)) if *n >= 0 => *n as usize,
            _ => return Err("the expected count is undetermined"),
        };
        let mut chain: Vec<FormKey> = Vec::new();
        while let Some(to) = self.linked(&at, &kw)? {
            if to == at || chain.contains(&to) {
                return Err("the chain loops");
            }
            if chain.len() >= max {
                return Err("the chain is longer than the expected count");
            }
            chain.push(to.clone());
            at = to;
        }
        let kind = if chain.is_empty() {
            Answer::NoRecord
        } else {
            Answer::Record
        };
        if count {
            return Ok((Val::Int(chain.len() as i32), Answer::Value));
        }
        let mut items = Vec::new();
        for k in &chain {
            items.push(self.object(w, k, "objectreference")?);
        }
        Ok((w.array("objectreference", items), kind))
    }
    fn is_disabled(&mut self, c: &NativeCall, w: &mut World) -> Got {
        let (o, k) =
            Self::record_of(w, c.receiver.as_ref()).ok_or("the receiver is not an object")?;
        self.disabled_of(w, Some(o), k)
            .map(|d| (Val::Bool(d), Answer::Value))
    }
    /// `IsDisabled` of a reference: this run's own `Enable` or `Disable`
    /// of its handle `o`, else its enable parent chain or header flag.
    fn disabled_of(
        &mut self,
        w: &World,
        o: Option<ObjId>,
        k: Option<FormKey>,
    ) -> Result<bool, &'static str> {
        if let Some(d) = o.and_then(|o| self.disabled.get(&o)) {
            return Ok(*d);
        }
        let k = k.ok_or("the receiver is not a record")?;
        let ov = self
            .store
            .overrides()
            .winner(&k)
            .ok_or("the reference names no winning record")?;
        if self.enable_chain {
            let b = Budget::new(self.max_memory);
            let chain = fo4_plugin::enable::chain(self.store, &k, &b)
                .map_err(|_| "the enable parent chain could not be read")?;
            if let Some(ch) = chain.filter(|c| c.depth() > 0) {
                // This run's own Enable and Disable of the chain's records
                // (`xesp_script_change_propagates_v1`).
                let mut set: HashMap<FormKey, bool> = HashMap::new();
                for n in &ch.nodes {
                    let key = record_key(&n.key);
                    let o = w
                        .objects
                        .iter()
                        .position(|x| x.key.as_deref() == Some(&key));
                    if let Some(d) = o.and_then(|o| self.disabled.get(&o)) {
                        set.insert(n.key.clone(), !*d);
                    }
                }
                let (on, used) = chain_state_with(&ch, &set, self.child_flag_ignored)?;
                self.using(rules::IS_DISABLED_CHAIN);
                for u in used {
                    self.using(u);
                }
                return Ok(!on);
            }
        }
        Ok(ov.disabled)
    }
    fn base_object(&mut self, c: &NativeCall, w: &mut World) -> Got {
        let (o, k) =
            Self::record_of(w, c.receiver.as_ref()).ok_or("the receiver is not an object")?;
        let placed = w.placement_bases.iter().find(|x| x.0 == o);
        let placed = placed.and_then(|x| key_record(&x.1));
        let base = match (k, placed) {
            (_, Some(b)) => b,
            (Some(k), None) => {
                let b = Budget::new(self.max_memory);
                crate::bind::base_of(self.store, &k, &b)
                    .ok()
                    .flatten()
                    .ok_or("the reference has no base")?
            }
            (None, None) => return Err("the receiver is not a record"),
        };
        Ok((self.object(w, &base, &c.return_type)?, Answer::Record))
    }
    fn form_from_file(&mut self, c: &NativeCall, w: &mut World) -> Got {
        let (Some(Val::Int(id)), Some(Val::Str(file))) = (c.args.first(), c.args.get(1)) else {
            return Err("an argument is undetermined");
        };
        let h = self
            .store
            .order()
            .headers()
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(file));
        let Some(h) = h else {
            return Ok((Val::None, Answer::NoRecord));
        };
        let mask = if h.is_light() { 0xfff } else { 0x00ff_ffff };
        let k = FormKey {
            plugin: h.name.to_ascii_lowercase(),
            object_id: (*id as u32) & mask,
        };
        if self.store.signature(&k).is_none() {
            return Ok((Val::None, Answer::NoRecord));
        }
        Ok((self.object(w, &k, &c.return_type)?, Answer::Record))
    }
    fn global_value(&mut self, c: &NativeCall, w: &mut World) -> Got {
        let (o, k) =
            Self::record_of(w, c.receiver.as_ref()).ok_or("the receiver is not an object")?;
        if self.changed.contains(&(o, "value")) {
            return Err("changed by this run");
        }
        let k = k.ok_or("the receiver is not a record")?;
        let v = global_fltv(self.store, &k, self.max_memory)?;
        Ok((Val::Float(v), Answer::Value))
    }
    /// Whether a native of this run may have changed the value of the
    /// global or form list `k` (so the data no longer answers it).
    pub fn value_changed(&self, w: &World, k: &FormKey) -> bool {
        // The changed facts are few; the world's objects many.
        let key = record_key(k);
        self.changed.iter().any(|(o, fact)| {
            *fact == "value"
                && w.objects
                    .get(*o)
                    .is_some_and(|x| x.key.as_deref() == Some(&key))
        })
    }
    /// The entries of a form list (`vm_host_formlist_lnam_v1`): its LNAM
    /// subrecords in stored order.
    fn entries(&self, c: &NativeCall, w: &World) -> Result<Vec<Option<FormKey>>, &'static str> {
        let (o, k) =
            Self::record_of(w, c.receiver.as_ref()).ok_or("the receiver is not an object")?;
        if self.changed.contains(&(o, "value")) {
            return Err("changed by this run");
        }
        let k = k.ok_or("the receiver is not a record")?;
        let (r, plugin) = self.raw(&k).ok_or("the list names no winning record")?;
        let mut out = Vec::new();
        for s in r.subrecords.iter().filter(|s| s.signature == *b"LNAM") {
            let d: [u8; 4] = r.data(s).try_into().map_err(|_| "LNAM is not 4 bytes")?;
            let e = self
                .store
                .order()
                .remap(plugin, u32::from_le_bytes(d), &r.source.file, s.offset)
                .map_err(|_| "an LNAM does not resolve")?;
            out.push(e);
        }
        Ok(out)
    }
    fn has_form(&mut self, c: &NativeCall, w: &mut World) -> Got {
        let entries = self.entries(c, w)?;
        let (_, f) = Self::record_of(w, c.args.first()).ok_or("the form is not an object")?;
        let f = f.ok_or("the form is not a record")?;
        if entries.iter().any(|e| e.as_ref() == Some(&f)) {
            return Ok((Val::Bool(true), Answer::Value));
        }
        let nested = entries
            .iter()
            .flatten()
            .any(|e| self.store.signature(e) == Some(*b"FLST"));
        if nested {
            return Err("the list holds lists");
        }
        Ok((Val::Bool(false), Answer::Value))
    }
    fn list_size(&mut self, c: &NativeCall, w: &mut World) -> Got {
        let n = self.entries(c, w)?.len();
        Ok((Val::Int(n as i32), Answer::Value))
    }
    fn list_at(&mut self, c: &NativeCall, w: &mut World) -> Got {
        let entries = self.entries(c, w)?;
        let Some(Val::Int(i)) = c.args.first() else {
            return Err("the index is undetermined");
        };
        let e = usize::try_from(*i)
            .ok()
            .and_then(|i| entries.get(i))
            .ok_or("the index is outside the list")?;
        match e {
            None => Ok((Val::None, Answer::NoRecord)),
            Some(k) if self.store.signature(k).is_none() => {
                Err("the entry names no winning record")
            }
            Some(k) => Ok((self.object(w, k, &c.return_type)?, Answer::Record)),
        }
    }
    /// Marks what a native other than the data host's own may change.
    fn note_changes(&mut self, c: &NativeCall, w: &World) {
        let full = c.full_name();
        if let Some((o, _)) = Self::record_of(w, c.receiver.as_ref()) {
            match full.as_str() {
                "objectreference.enable" => {
                    self.disabled.insert(o, false);
                }
                "objectreference.disable" | "objectreference.disablenowait" => {
                    self.disabled.insert(o, true);
                }
                "objectreference.setlinkedref" => {
                    self.changed.insert((o, "linked"));
                }
                _ => {}
            }
        }
        if PURE_READS.contains(&full.as_str()) {
            return;
        }
        // A global or form list given to any other native, or receiving
        // it, may be changed by it.
        let args = c.receiver.iter().chain(&c.args);
        for v in args {
            if let Val::Object(o) = v {
                let ob = &w.objects[*o];
                let is = |t: &str| {
                    ob.types.iter().any(|x| x == t) || ob.signature_type.as_deref() == Some(t)
                };
                if is("globalvariable") || is("formlist") {
                    self.changed.insert((*o, "value"));
                }
            }
        }
    }
}

impl Host for DataHost<'_> {
    fn call(&mut self, c: &NativeCall, w: &mut World) -> Reply {
        let full = c.full_name();
        let Some(&(_, rule)) = NATIVES.iter().find(|(n, _)| *n == full) else {
            self.note_changes(c, w);
            let r = self.recording.call(c, w);
            for u in self.recording.used.clone() {
                self.using(u);
            }
            return r;
        };
        let got = match c.name.as_str() {
            "getlinkedref" => self.get_linked_ref(c, w),
            "getlinkedrefchain" => self.linked_chain(c, w, false),
            "countlinkedrefchain" => self.linked_chain(c, w, true),
            "isdisabled" => self.is_disabled(c, w),
            "getbaseobject" => self.base_object(c, w),
            "getformfromfile" => self.form_from_file(c, w),
            "getvalue" => self.global_value(c, w),
            "hasform" => self.has_form(c, w),
            "getsize" => self.list_size(c, w),
            "getat" => self.list_at(c, w),
            _ => Err("not a data native"),
        };
        let (reply, answer) = match got {
            Ok((v, a)) => {
                self.using(rule);
                self.using(rules::DATA_STATE);
                (Reply::Value(v), a)
            }
            Err(why) => (Reply::Undetermined, Answer::Open(why)),
        };
        *self.answers.entry((full, answer)).or_default() += 1;
        reply
    }
    fn rules(&self) -> &[&'static str] {
        &self.used
    }
    fn reference_disabled(
        &mut self,
        w: &mut World,
        k: &FormKey,
    ) -> Option<Result<bool, &'static str>> {
        let key = record_key(k);
        let o = w
            .objects
            .iter()
            .position(|x| x.key.as_deref() == Some(&key));
        let r = self.disabled_of(w, o, Some(k.clone()));
        if r.is_ok() {
            self.using(rules::IS_DISABLED);
            self.using(rules::DATA_STATE);
        }
        Some(r)
    }
}
