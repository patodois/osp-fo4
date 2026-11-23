#![forbid(unsafe_code)]
//! A small interpreter over compiled Fallout 4 scripts (`fo4_pex`), run
//! under pending rules (`spec/vm.md`).
//!
//! What each opcode does is not established by any measurement of the
//! running game: every opcode runs under the candidate meaning registered
//! as a pending rule in `spec/hypotheses.tsv` ([`rules::of_opcode`]), and a
//! run lists the rules it used ([`Vm::rules`]). An opcode without a
//! registered meaning stops the run with [`VmError::UnmeasuredOpcode`].
//!
//! Native functions are not implemented as game behaviour. The interpreter
//! writes every native call to an effect log ([`Effect`]) and asks a
//! [`Host`] for the result; the [`RecordingHost`] answers with a declared
//! default only where `spec/vm.md` lists one, and with an undetermined
//! value otherwise. An undetermined value may be stored, passed and logged;
//! where it would steer control flow (a conditional jump, a scripted call
//! or property on it, an array index or size) the run stops as
//! [`Outcome::Blocked`], naming what it depends on. Nothing is guessed and
//! nothing is skipped: every other stop is a typed [`VmError`].
//!
//! Runs are deterministic, bounded in steps and call depth, and
//! single-threaded (a native that waits returns at once).
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    rc::Rc,
};

pub mod anim;
pub mod bind;
pub mod blockers;
pub mod cond;
pub mod data;
pub mod events;
mod exec;
pub mod host;
pub mod keyword;
pub mod load;
pub mod menu;
pub mod player;
pub mod program;
pub mod quest;
pub mod rules;
pub mod scene;
pub mod setters;
pub mod sit;
pub mod story;
pub mod talk;
pub mod track;
pub mod vars;
pub mod voice;
pub mod world;

pub use host::{Host, NativeCall, NoHost, RecordingHost, Reply};
pub use program::{Func, Ins, Library, Lit, MapLibrary, Script};
pub use world::{InstId, ObjId, Origin, Val, World};

/// Where an instruction is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub script: String,
    pub state: String,
    pub function: String,
    pub index: usize,
    pub op: u8,
}
impl fmt::Display for Site {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}{} #{} op {}",
            self.script,
            if self.state.is_empty() {
                String::new()
            } else {
                format!("[{}].", self.state)
            },
            self.function,
            self.index,
            self.op
        )
    }
}

/// Why a run stopped other than by completing or blocking.
#[derive(Debug, Clone, PartialEq)]
pub enum VmError {
    /// An opcode without a registered candidate meaning.
    UnmeasuredOpcode {
        site: Box<Site>,
    },
    MissingScript(String),
    MissingFunction {
        script: String,
        name: String,
    },
    MissingProperty {
        script: String,
        name: String,
    },
    /// The host does not provide a native function.
    NativeNotProvided(String),
    TypeMismatch {
        site: Option<Box<Site>>,
        expected: String,
        found: String,
    },
    StepLimit(u64),
    DepthLimit(usize),
    IndexOutOfRange {
        site: Box<Site>,
        index: i64,
        len: usize,
    },
    DivisionByZero {
        site: Box<Site>,
    },
    /// A call, property, array or struct operation on none.
    NoneOperand {
        site: Box<Site>,
        what: String,
    },
    /// An identifier that names nothing in the function, its instance or
    /// the instance's scripts.
    UnknownName {
        site: Box<Site>,
        name: String,
    },
    /// A slot that cannot hold what the candidate meaning needs (for
    /// example a literal as a destination).
    BadArgument {
        site: Option<Box<Site>>,
        detail: String,
    },
    /// Two script instances of one object answer a call differently.
    Ambiguous {
        site: Box<Site>,
        detail: String,
    },
    /// A timer firing that the queued timers do not order.
    Timers(String),
}
impl fmt::Display for VmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let site =
            |s: &Option<Box<Site>>| s.as_ref().map(|s| format!(" at {s}")).unwrap_or_default();
        match self {
            VmError::UnmeasuredOpcode { site } => write!(f, "UnmeasuredOpcode at {site}"),
            VmError::MissingScript(s) => write!(f, "MissingScript {s}"),
            VmError::MissingFunction { script, name } => {
                write!(f, "MissingFunction {script}.{name}")
            }
            VmError::MissingProperty { script, name } => {
                write!(f, "MissingProperty {script}.{name}")
            }
            VmError::NativeNotProvided(n) => write!(f, "NativeNotProvided {n}"),
            VmError::TypeMismatch {
                site: s,
                expected,
                found,
            } => write!(
                f,
                "TypeMismatch{}: expected {expected}, found {found}",
                site(s)
            ),
            VmError::StepLimit(n) => write!(f, "StepLimit {n}"),
            VmError::DepthLimit(n) => write!(f, "DepthLimit {n}"),
            VmError::IndexOutOfRange { site, index, len } => {
                write!(f, "IndexOutOfRange at {site}: {index} of {len}")
            }
            VmError::DivisionByZero { site } => write!(f, "DivisionByZero at {site}"),
            VmError::NoneOperand { site, what } => write!(f, "NoneOperand at {site}: {what}"),
            VmError::UnknownName { site, name } => write!(f, "UnknownName at {site}: {name}"),
            VmError::BadArgument { site: s, detail } => {
                write!(f, "BadArgument{}: {detail}", site(s))
            }
            VmError::Ambiguous { site, detail } => write!(f, "Ambiguous at {site}: {detail}"),
            VmError::Timers(d) => write!(f, "Timers: {d}"),
        }
    }
}
impl std::error::Error for VmError {}
impl VmError {
    /// A short kind name for aggregates.
    pub fn kind(&self) -> String {
        match self {
            VmError::UnmeasuredOpcode { site } => format!("UnmeasuredOpcode {}", site.op),
            VmError::MissingScript(_) => "MissingScript".into(),
            VmError::MissingFunction { .. } => "MissingFunction".into(),
            VmError::MissingProperty { .. } => "MissingProperty".into(),
            VmError::NativeNotProvided(_) => "NativeNotProvided".into(),
            VmError::TypeMismatch { site, .. } => format!(
                "TypeMismatch{}",
                site.as_ref()
                    .map(|s| format!(" op {}", s.op))
                    .unwrap_or_default()
            ),
            VmError::StepLimit(_) => "StepLimit".into(),
            VmError::DepthLimit(_) => "DepthLimit".into(),
            VmError::IndexOutOfRange { site, .. } => format!("IndexOutOfRange op {}", site.op),
            VmError::DivisionByZero { .. } => "DivisionByZero".into(),
            VmError::NoneOperand { site, .. } => format!("NoneOperand op {}", site.op),
            VmError::UnknownName { .. } => "UnknownName".into(),
            VmError::BadArgument { .. } => "BadArgument".into(),
            VmError::Ambiguous { .. } => "Ambiguous".into(),
            VmError::Timers(_) => "Timers".into(),
        }
    }
}

/// Where and on what a run blocked.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub on: Origin,
    pub at: Option<Site>,
}

/// How a delivery or call ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Completed(Val),
    Blocked(Block),
    Failed(VmError),
}

/// A blocked or failed run on its way out (boxed: it crosses every nested
/// call).
pub(crate) enum Stop {
    Blocked(Box<Block>),
    Error(Box<VmError>),
}
impl Stop {
    pub(crate) fn err(e: VmError) -> Self {
        Stop::Error(Box::new(e))
    }
    pub(crate) fn blocked(b: Block) -> Self {
        Stop::Blocked(Box::new(b))
    }
}
impl From<VmError> for Stop {
    fn from(e: VmError) -> Self {
        Stop::err(e)
    }
}
pub(crate) type R<T> = Result<T, Stop>;

/// One native call, as logged.
#[derive(Debug, Clone)]
pub struct Effect {
    /// Call depth of the calling function (0 = the delivered handler).
    pub depth: usize,
    /// `script.function` of the caller.
    pub caller: String,
    /// `script.function` of the native, lower case.
    pub native: String,
    pub receiver: Option<String>,
    /// The receiver when it is an object handle.
    pub receiver_object: Option<ObjId>,
    pub args: Vec<String>,
    pub result: String,
}

/// Bounds of one delivery.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Instructions run.
    pub steps: u64,
    /// Nested scripted calls.
    pub depth: usize,
    /// Elements of one array.
    pub array_len: usize,
    /// Arrays, structs and undetermined values created in one run.
    pub values: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Limits {
            steps: 200_000,
            depth: 64,
            array_len: 4096,
            values: 1 << 20,
        }
    }
}

/// Readings of the array opcodes on a none array, each a pending rule the
/// caller turns on (`quest_run --none-array-*`). All off by default: the
/// length of none is undetermined and every other array opcode fails on
/// none, as before.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoneArrays {
    /// Opcode 31 on none stores 0 (`pex_array_none_length_v1`).
    pub length: bool,
    /// Opcode 32 on none stores the default of the element type and the
    /// run goes on (`pex_array_none_read_v1`).
    pub read: bool,
    /// Opcodes 34 and 40 on none store -1 (`pex_array_none_find_v1`).
    pub find: bool,
    /// Opcodes 33, 42, 43, 45 and 46 on none change nothing and the run
    /// goes on (`pex_array_none_change_v1`).
    pub change: bool,
}

/// Alternative readings of opcodes, for the checks that could refute the
/// candidate meanings (`vm_checks`). [`Variant::Candidate`] is the
/// registered reading; the others each exchange one pair of meanings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Variant {
    Candidate,
    /// 21 jumps when false, 22 when true.
    JumpSenseSwapped,
    /// 16 and 18, 17 and 19 exchanged (less and greater).
    LessGreaterSwapped,
    /// 16 and 17, 18 and 19 exchanged (strict and not strict).
    StrictSwapped,
    /// 15 stores inequality.
    EqualNegated,
    /// 1 and 3, 2 and 4 exchanged (sum and difference).
    AddSubSwapped,
    /// 5 and 7, 6 and 8 exchanged (product and quotient).
    MulDivSwapped,
    /// 28 writes, 29 reads.
    PropertySwapped,
    /// 14 copies without converting, as 13.
    CastAsCopy,
}
impl Variant {
    pub const ALL: [Variant; 9] = [
        Variant::Candidate,
        Variant::JumpSenseSwapped,
        Variant::LessGreaterSwapped,
        Variant::StrictSwapped,
        Variant::EqualNegated,
        Variant::AddSubSwapped,
        Variant::MulDivSwapped,
        Variant::PropertySwapped,
        Variant::CastAsCopy,
    ];
    /// The opcode whose candidate meaning runs for a stored opcode.
    pub fn map(self, op: u8) -> u8 {
        let swap = |a: u8, b: u8| {
            if op == a {
                b
            } else if op == b {
                a
            } else {
                op
            }
        };
        match self {
            Variant::Candidate | Variant::EqualNegated => op,
            Variant::JumpSenseSwapped => swap(21, 22),
            Variant::LessGreaterSwapped => match op {
                16 | 18 => swap(16, 18),
                _ => swap(17, 19),
            },
            Variant::StrictSwapped => match op {
                16 | 17 => swap(16, 17),
                _ => swap(18, 19),
            },
            Variant::AddSubSwapped => match op {
                1 | 3 => swap(1, 3),
                _ => swap(2, 4),
            },
            Variant::MulDivSwapped => match op {
                5 | 7 => swap(5, 7),
                _ => swap(6, 8),
            },
            Variant::PropertySwapped => swap(28, 29),
            Variant::CastAsCopy => {
                if op == 14 {
                    13
                } else {
                    op
                }
            }
        }
    }
}

/// Attaches the scripts of the record behind an object when a run first
/// needs an instance on it (`vm_record_scripts_attach_v1`): asked once per
/// object, before the object's instances are looked up for a call, a
/// property, a cast or a type test.
pub trait Attach {
    fn attach(&mut self, vm: &mut Vm<'_>, o: ObjId) -> Result<(), VmError>;
}

/// Runs the stage fragments and quest events a native queued, at once,
/// inside the run that called the native (`quest_stage_fragment_order_v1`,
/// nested reading): asked after each native call that left the quest
/// queue non-empty. `None` leaves them queued for `quest::run_jobs`.
pub trait Nested {
    fn run(&mut self, vm: &mut Vm<'_>);
}

/// The interpreter.
pub struct Vm<'a> {
    lib: &'a mut dyn Library,
    pub host: &'a mut dyn Host,
    /// Attaches the scripts of records on first need; `None` attaches
    /// nothing.
    pub attach: Option<&'a mut dyn Attach>,
    /// Runs queued quest jobs inside the calling run; `None` leaves them
    /// queued.
    pub nested: Option<&'a mut dyn Nested>,
    scripts: BTreeMap<String, Option<Rc<Script>>>,
    pub world: World,
    pub effects: Vec<Effect>,
    /// Rules the runs so far relied on.
    pub rules: BTreeSet<&'static str>,
    pub limits: Limits,
    pub variant: Variant,
    /// Readings of array opcodes on a none array, each off by default.
    pub none_arrays: NoneArrays,
    steps: u64,
    depth: usize,
}

/// Longest parent chain followed.
pub const MAX_PARENTS: usize = 64;

impl<'a> Vm<'a> {
    pub fn new(lib: &'a mut dyn Library, host: &'a mut dyn Host) -> Self {
        Vm {
            lib,
            host,
            attach: None,
            nested: None,
            scripts: BTreeMap::new(),
            world: World::default(),
            effects: Vec::new(),
            rules: BTreeSet::new(),
            limits: Limits::default(),
            variant: Variant::Candidate,
            none_arrays: NoneArrays::default(),
            steps: 0,
            depth: 0,
        }
    }
    /// A script by lower-case name, `None` when the library has none.
    pub fn try_script(&mut self, name: &str) -> Result<Option<Rc<Script>>, VmError> {
        if let Some(s) = self.scripts.get(name) {
            return Ok(s.clone());
        }
        let s = self.lib.load(name)?;
        self.scripts.insert(name.to_string(), s.clone());
        Ok(s)
    }
    pub fn script(&mut self, name: &str) -> Result<Rc<Script>, VmError> {
        self.try_script(name)?
            .ok_or_else(|| VmError::MissingScript(name.to_string()))
    }
    /// The script and its parents, nearest first.
    pub fn chain(&mut self, name: &str) -> Result<Vec<Rc<Script>>, VmError> {
        let mut out = Vec::new();
        let mut t = name.to_string();
        loop {
            let s = self.script(&t)?;
            t = s.parent.clone();
            out.push(s);
            if t.is_empty() {
                return Ok(out);
            }
            if out.len() >= MAX_PARENTS {
                return Err(VmError::BadArgument {
                    site: None,
                    detail: format!("parent chain of {name} longer than {MAX_PARENTS}"),
                });
            }
        }
    }
    /// Whether `type_name` is `target` or has it as a parent; false for
    /// names that are not scripts.
    pub fn is_a(&mut self, type_name: &str, target: &str) -> Result<bool, VmError> {
        if type_name == target {
            return Ok(true);
        }
        if self.try_script(type_name)?.is_none() {
            return Ok(false);
        }
        Ok(self.chain(type_name)?.iter().any(|s| s.name == target))
    }
    /// Whether an object is known to be of a type: one of its known types
    /// or attached scripts is the type or has it as a parent.
    pub fn object_is_a(&mut self, o: ObjId, target: &str) -> Result<bool, VmError> {
        self.attach_scripts(o)?;
        let mut types = self.world.objects[o].types.clone();
        for i in self.world.objects[o].instances.clone() {
            types.push(self.world.instances[i].script.clone());
        }
        for t in types {
            if self.is_a(&t, target)? {
                return Ok(true);
            }
        }
        if let Some(t) = self.world.objects[o].signature_type.clone() {
            if self.is_a(&t, target)? {
                self.rules.insert(rules::SIGNATURE_TYPE);
                return Ok(true);
            }
        }
        Ok(false)
    }
    /// Asks [`Vm::attach`] once for an object.
    pub fn attach_scripts(&mut self, o: ObjId) -> Result<(), VmError> {
        if self.world.objects[o].attached {
            return Ok(());
        }
        self.world.objects[o].attached = true;
        if let Some(a) = self.attach.take() {
            let r = a.attach(self, o);
            self.attach = Some(a);
            r?;
        }
        Ok(())
    }
    /// A new instance of a script (and its parents' variables), in the
    /// auto state of the script, attached to an object when given.
    pub fn instantiate(&mut self, object: Option<ObjId>, script: &str) -> Result<InstId, VmError> {
        let chain = self.chain(script)?;
        let mut vars = Vec::new();
        for s in &chain {
            for v in &s.variables {
                let value = self.initial(&v.init, &v.type_name);
                vars.push(world::VarSlot {
                    script: s.name.clone(),
                    name: v.name.clone(),
                    type_name: v.type_name.clone(),
                    value,
                });
            }
        }
        let state = chain[0].auto_state.clone();
        if !state.is_empty() {
            self.rules.insert(rules::STATE);
        }
        self.world.instances.push(world::Instance {
            object,
            script: script.to_string(),
            state,
            vars,
        });
        let id = self.world.instances.len() - 1;
        if let Some(o) = object {
            self.world.objects[o].instances.push(id);
        }
        Ok(id)
    }
    /// Value of a stored initial value in a slot of a type: none in a
    /// slot of a simple type takes that type's default (`pex_default_values_v1`).
    pub(crate) fn initial(&mut self, l: &Lit, type_name: &str) -> Val {
        match l {
            Lit::None => self.default_of(type_name),
            Lit::Int(n) if type_name == "float" => Val::Float(*n as f32),
            Lit::Int(n) => Val::Int(*n),
            Lit::Float(f) => Val::Float(*f),
            Lit::Bool(b) => Val::Bool(*b),
            Lit::Str(s) => Val::Str(s.clone()),
            Lit::Ident(_) => Val::None,
        }
    }
    pub(crate) fn default_of(&mut self, type_name: &str) -> Val {
        let v = match type_name {
            "int" => Val::Int(0),
            "float" => Val::Float(0.0),
            "bool" => Val::Bool(false),
            "string" => Val::Str(String::new()),
            _ => return Val::None,
        };
        self.rules.insert(rules::DEFAULTS);
        v
    }
    /// Sets the variable behind an instance's property (for binding);
    /// `Ok(false)` when the property has no variable behind it.
    pub fn set_property_variable(
        &mut self,
        inst: InstId,
        name: &str,
        v: Val,
    ) -> Result<bool, VmError> {
        let script = self.world.instances[inst].script.clone();
        for s in self.chain(&script)? {
            if let Some(p) = s.property(name) {
                let Some(var) = &p.variable else {
                    return Ok(false);
                };
                let ty = p.type_name.clone();
                if !self.fits(&v, &ty)? {
                    return Err(VmError::TypeMismatch {
                        site: None,
                        expected: ty,
                        found: self.kind_of(&v),
                    });
                }
                let slot = self.world.instances[inst]
                    .vars
                    .iter_mut()
                    .find(|x| x.script == s.name && x.name == *var)
                    .ok_or_else(|| VmError::MissingProperty {
                        script: s.name.clone(),
                        name: var.clone(),
                    })?;
                slot.value = v;
                return Ok(true);
            }
        }
        Err(VmError::MissingProperty {
            script,
            name: name.to_string(),
        })
    }
    /// The type of a declared property of a script or a parent.
    pub fn property_type(&mut self, script: &str, name: &str) -> Result<Option<String>, VmError> {
        Ok(self
            .chain(script)?
            .iter()
            .find_map(|s| s.property(name).map(|p| p.type_name.clone())))
    }
    /// Delivers an event: runs the function of that name on the instance,
    /// dispatched in its current state.
    pub fn deliver(&mut self, inst: InstId, event: &str, args: Vec<Val>) -> Outcome {
        self.steps = 0;
        self.depth = 0;
        let r = (|| {
            let f = self
                .dispatch(inst, event)?
                .ok_or_else(|| VmError::MissingFunction {
                    script: self.world.instances[inst].script.clone(),
                    name: event.to_string(),
                })?;
            if f.native() {
                return Err(Stop::err(VmError::BadArgument {
                    site: None,
                    detail: format!("event {event} is native"),
                }));
            }
            self.call(Some(inst), f, args)
        })();
        Self::outcome(r)
    }
    /// Delivers an event inside a running delivery (a nested quest job):
    /// as [`Vm::deliver`], but the step and call-depth counts go on from
    /// the enclosing run's, so its limits bound both.
    pub fn deliver_nested(&mut self, inst: InstId, event: &str, args: Vec<Val>) -> Outcome {
        let depth = self.depth;
        let r = (|| {
            let f = self
                .dispatch(inst, event)?
                .ok_or_else(|| VmError::MissingFunction {
                    script: self.world.instances[inst].script.clone(),
                    name: event.to_string(),
                })?;
            self.call(Some(inst), f, args)
        })();
        self.depth = depth;
        Self::outcome(r)
    }
    /// Runs one scripted function (a check's entry point), with an
    /// instance for `self` when given.
    pub fn run(&mut self, inst: Option<InstId>, f: Rc<Func>, args: Vec<Val>) -> Outcome {
        self.steps = 0;
        self.depth = 0;
        let r = self.call(inst, f, args);
        Self::outcome(r)
    }
    fn outcome(r: R<Val>) -> Outcome {
        match r {
            Ok(v) => Outcome::Completed(v),
            Err(Stop::Blocked(b)) => Outcome::Blocked(*b),
            Err(Stop::Error(e)) => Outcome::Failed(*e),
        }
    }
    /// Fires queued timers in due order, at most `max`, delivering
    /// `OnTimer` or `OnTimerGameTime` with the timer's id
    /// (`vm_timer_events_v1`). Stops with an error when the queue does not
    /// order them: an undetermined interval, both clocks queued, or two
    /// timers of one id on one instance and clock.
    pub fn fire_timers(&mut self, max: usize) -> Vec<(InstId, Val, Outcome)> {
        let mut out = Vec::new();
        for _ in 0..max {
            if self.world.timers.is_empty() {
                break;
            }
            let t = &self.world.timers;
            let problem = if t.iter().any(|x| x.due.is_none()) {
                Some("a timer interval is undetermined")
            } else if t.iter().any(|x| x.clock != t[0].clock) {
                Some("timers on both clocks are queued")
            } else if t.iter().enumerate().any(|(i, x)| {
                t[i + 1..]
                    .iter()
                    .any(|y| y.instance == x.instance && y.clock == x.clock && y.id == x.id)
            }) {
                Some("two timers of one id on one instance are queued")
            } else {
                None
            };
            if let Some(p) = problem {
                let first = t[0].instance;
                out.push((
                    first,
                    Val::None,
                    Outcome::Failed(VmError::Timers(p.to_string())),
                ));
                break;
            }
            let i = (0..t.len())
                .min_by(|&a, &b| {
                    t[a].due
                        .partial_cmp(&t[b].due)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(t[a].seq.cmp(&t[b].seq))
                })
                .unwrap_or(0);
            let timer = self.world.timers.remove(i);
            self.rules.insert(rules::TIMERS);
            self.world.now[timer.clock as usize] = timer.due.unwrap_or(0.0);
            let event = match timer.clock {
                world::Clock::Real => "ontimer",
                world::Clock::GameTime => "ontimergametime",
            };
            let o = self.deliver(timer.instance, event, vec![timer.id.clone()]);
            out.push((timer.instance, timer.id, o));
        }
        out
    }
    /// Fires the next timer under `vm_timer_time_scale_v1`: a game-time
    /// timer's interval in hours is due after `hours * 3600 / scale` real
    /// seconds; with a scale of 0 the game clock stands still and no
    /// game-time timer is due. Both clocks advance together. `scale` is the
    /// caller's reading of the time-scale global (`None`: undetermined,
    /// which stops the firing as [`Vm::fire_timers`] does). Returns `None`
    /// when no timer is due.
    pub fn fire_timer_scaled(&mut self, scale: Option<f64>) -> Option<(InstId, Val, Outcome)> {
        use world::Clock;
        let first = self.world.timers.first()?.instance;
        let (i, dt) = match self.pick_timer(scale) {
            Err(p) => return Some((first, Val::None, Outcome::Failed(VmError::Timers(p.into())))),
            Ok(x) => x?,
        };
        let scaled = scale.is_some();
        let scale = scale.unwrap_or(1.0);
        let now_real = self.world.now[Clock::Real as usize];
        let now_game = self.world.now[Clock::GameTime as usize];
        let timer = self.world.timers.remove(i);
        self.rules.insert(rules::TIMERS);
        if scaled {
            self.rules.insert(rules::TIME_SCALE);
            self.world.now[Clock::Real as usize] = now_real + dt;
            self.world.now[Clock::GameTime as usize] = now_game + dt * scale / 3600.0;
        }
        self.world.now[timer.clock as usize] = timer.due.unwrap_or(0.0);
        let event = match timer.clock {
            Clock::Real => "ontimer",
            Clock::GameTime => "ontimergametime",
        };
        let o = self.deliver(timer.instance, event, vec![timer.id.clone()]);
        Some((timer.instance, timer.id, o))
    }
    /// Real seconds until the timer [`Vm::fire_timer_scaled`] would fire
    /// next, under the same reading; `None` when it would fire none or
    /// fail.
    pub fn next_timer_wait(&self, scale: Option<f64>) -> Option<f64> {
        self.pick_timer(scale).ok().flatten().map(|(_, dt)| dt)
    }
    /// The timer due first and the real seconds until it is due, or why
    /// the queue cannot be ordered.
    fn pick_timer(&self, scale: Option<f64>) -> Result<Option<(usize, f64)>, &'static str> {
        use world::Clock;
        let t = &self.world.timers;
        if t.is_empty() {
            return Ok(None);
        }
        if t.iter().any(|x| x.due.is_none()) {
            return Err("a timer interval is undetermined");
        }
        let both = t.iter().any(|x| x.clock != t[0].clock);
        // With a scale the clocks are related for every timer; without one
        // only a queue on one clock is ordered.
        let scaled = scale.is_some();
        let Some(scale) = scale.or((!both).then_some(1.0)) else {
            return Err("timers on both clocks are queued and the time scale is undetermined");
        };
        if t.iter().enumerate().any(|(i, x)| {
            t[i + 1..]
                .iter()
                .any(|y| y.instance == x.instance && y.clock == x.clock && y.id == x.id)
        }) {
            return Err("two timers of one id on one instance are queued");
        }
        let now_real = self.world.now[Clock::Real as usize];
        let now_game = self.world.now[Clock::GameTime as usize];
        // Real seconds from now until each timer is due.
        let wait = |x: &world::Timer| -> Option<f64> {
            let due = x.due.unwrap_or(0.0);
            match x.clock {
                Clock::Real => Some(due - now_real),
                Clock::GameTime if !scaled => Some(due - now_game),
                Clock::GameTime if scale > 0.0 => Some((due - now_game) * 3600.0 / scale),
                Clock::GameTime => None,
            }
        };
        let i = (0..t.len())
            .filter(|&a| wait(&t[a]).is_some())
            .min_by(|&a, &b| {
                wait(&t[a])
                    .partial_cmp(&wait(&t[b]))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(t[a].seq.cmp(&t[b].seq))
            });
        Ok(i.map(|i| (i, wait(&t[i]).unwrap_or(0.0).max(0.0))))
    }
}

#[cfg(test)]
mod tests;
