//! A static reading of a movie's AVM2 blocks (DoAbc2), through the `swf`
//! crate's ABC reader: the classes with their members, and the call sites
//! that leave the movie (`spec/ui.md`, Host interface). Nothing is run.
//!
//! What counts as a call on the host is a pending reading
//! (`ui_host_calls_static_v1`): a call through `flash.external`'s
//! `ExternalInterface.call` with a constant name, a call to the global
//! `fscommand` with a constant name, and a call through a member that the
//! movie declares untyped (or as `Object`), never gives a value of its own
//! (an empty object, null or undefined stored there is a placeholder, and a
//! method's parameter stored there is a value handed on from its caller)
//! and never places by instance name, and that the movie calls through: an
//! object the host fills in.
//! What counts as an entry point is the public instance methods of a class
//! that declares such a member, its own and those it inherits from classes
//! of the same block (`ui_host_entry_public_v1`).
use fo4_core::{Error, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use swf::avm2::read::Reader;
use swf::avm2::types::{AbcFile, Index, Multiname, Namespace, Op, Trait, TraitKind};
use swf::extensions::ReadSwfExt;

/// Namespace class of a member's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Access {
    Public,
    Private,
    Protected,
    Internal,
    Other,
}

/// Kind of a class member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MemberKind {
    Method,
    Getter,
    Setter,
    Slot,
    Const,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    pub access: Access,
    pub kind: MemberKind,
    /// Slot or constant type, `*` when untyped.
    pub type_name: String,
    /// Method parameters.
    pub params: u32,
    pub is_static: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Class {
    /// Package and name joined with a dot (no dot in the root package).
    pub name: String,
    pub super_name: String,
    pub members: Vec<Member>,
}

/// How a call leaves the movie.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Via {
    /// `ExternalInterface.call` with a constant first argument.
    External,
    /// The global `fscommand` with a constant first argument.
    Command,
    /// A call through a member the host fills in (its name).
    Object(String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct HostCall {
    pub via: Via,
    /// The function called on the host.
    pub name: String,
    pub args: u32,
    /// Class and method holding the call site.
    pub class: String,
    pub method: String,
}

/// One block read.
#[derive(Debug, Clone, Default)]
pub struct Reading {
    pub classes: Vec<Class>,
    pub host_calls: Vec<HostCall>,
    /// Members the reading takes as host-filled objects (name, type).
    pub host_objects: Vec<(String, String)>,
    /// Classes declaring a host-filled member: the classes the host talks to.
    pub host_classes: Vec<String>,
    /// Call sites whose receiver the straight-line look-back could not name.
    pub unresolved_calls: u64,
    pub call_sites: u64,
    pub method_bodies: u64,
    /// Bodies whose code the reader refused part-way.
    pub refused_bodies: u64,
}

fn guarded<T>(file: &str, f: impl FnOnce() -> T) -> Result<T> {
    catch_unwind(AssertUnwindSafe(f))
        .map_err(|_| Error::new("ParserPanic", file, 0, "no panic", "ABC reader panicked"))
}

struct Pool<'a>(&'a AbcFile);
impl Pool<'_> {
    fn string(&self, i: Index<String>) -> String {
        match i.0.checked_sub(1) {
            Some(n) => self
                .0
                .constant_pool
                .strings
                .get(n as usize)
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .unwrap_or_default(),
            None => "*".into(),
        }
    }
    fn namespace(&self, i: Index<Namespace>) -> Option<&Namespace> {
        self.0
            .constant_pool
            .namespaces
            .get(i.0.checked_sub(1)? as usize)
    }
    fn multiname(&self, i: Index<Multiname>) -> Option<&Multiname> {
        self.0
            .constant_pool
            .multinames
            .get(i.0.checked_sub(1)? as usize)
    }
    /// The local name of a multiname; `None` for runtime names.
    /// A type name gives its base type's name; a chain of type names longer
    /// than the pool is a cycle and has no name.
    fn name(&self, i: Index<Multiname>) -> Option<String> {
        let mut i = i;
        for _ in 0..=self.0.constant_pool.multinames.len() {
            if i.0 == 0 {
                return Some("*".into());
            }
            match self.multiname(i)? {
                Multiname::QName { name, .. }
                | Multiname::QNameA { name, .. }
                | Multiname::Multiname { name, .. }
                | Multiname::MultinameA { name, .. }
                | Multiname::RTQName { name }
                | Multiname::RTQNameA { name } => return Some(self.string(*name)),
                Multiname::TypeName { base_type, .. } => i = Index::new(base_type.0),
                _ => return None,
            }
        }
        None
    }
    /// Operands a multiname takes from the stack at run time (Adobe's AVM2
    /// overview, multiname kinds: a runtime namespace one, a runtime name
    /// one).
    fn runtime(&self, i: Index<Multiname>) -> u32 {
        match self.multiname(i) {
            Some(Multiname::RTQName { .. } | Multiname::RTQNameA { .. }) => 1,
            Some(Multiname::RTQNameL | Multiname::RTQNameLA) => 2,
            Some(Multiname::MultinameL { .. } | Multiname::MultinameLA { .. }) => 1,
            _ => 0,
        }
    }
    fn qualified(&self, i: Index<Multiname>) -> String {
        let local = self.name(i).unwrap_or_else(|| "?".into());
        let pkg = match self.multiname(i) {
            Some(Multiname::QName { namespace, .. } | Multiname::QNameA { namespace, .. }) => {
                match self.namespace(*namespace) {
                    Some(Namespace::Package(p)) => self.string(*p),
                    _ => String::new(),
                }
            }
            _ => String::new(),
        };
        if pkg.is_empty() || pkg == "*" {
            local
        } else {
            format!("{pkg}.{local}")
        }
    }
    fn access(&self, i: Index<Multiname>) -> Access {
        let ns = match self.multiname(i) {
            Some(Multiname::QName { namespace, .. } | Multiname::QNameA { namespace, .. }) => {
                self.namespace(*namespace)
            }
            _ => None,
        };
        match ns {
            Some(Namespace::Package(_)) => Access::Public,
            Some(Namespace::Private(_)) => Access::Private,
            Some(Namespace::Protected(_) | Namespace::StaticProtected(_)) => Access::Protected,
            Some(Namespace::PackageInternal(_)) => Access::Internal,
            _ => Access::Other,
        }
    }
}

fn member(p: &Pool<'_>, t: &Trait, is_static: bool) -> Member {
    let params = |m: Index<swf::avm2::types::Method>| {
        p.0.methods
            .get(m.0 as usize)
            .map_or(0, |m| m.params.len() as u32)
    };
    let (kind, type_name, n) = match &t.kind {
        TraitKind::Method { method, .. } => (MemberKind::Method, String::new(), params(*method)),
        TraitKind::Getter { method, .. } => (MemberKind::Getter, String::new(), params(*method)),
        TraitKind::Setter { method, .. } => (MemberKind::Setter, String::new(), params(*method)),
        TraitKind::Slot { type_name, .. } => (
            MemberKind::Slot,
            p.name(*type_name).unwrap_or_else(|| "?".into()),
            0,
        ),
        TraitKind::Const { type_name, .. } => (
            MemberKind::Const,
            p.name(*type_name).unwrap_or_else(|| "?".into()),
            0,
        ),
        _ => (MemberKind::Other, String::new(), 0),
    };
    Member {
        name: p.name(t.name).unwrap_or_else(|| "?".into()),
        access: p.access(t.name),
        kind,
        type_name,
        params: n,
        is_static,
    }
}

/// Stack effect (pops, pushes) of the ops the look-back understands;
/// `None` stops it (control flow, runtime names, anything else).
fn effect(p: &Pool<'_>, op: &Op) -> Option<(u32, u32)> {
    use Op::*;
    let rt = |i: &Index<Multiname>| p.runtime(*i);
    let plus = |a: u32, i: &Index<Multiname>| a.checked_add(rt(i));
    Some(match op {
        PushByte { .. }
        | PushShort { .. }
        | PushInt { .. }
        | PushUint { .. }
        | PushDouble { .. }
        | PushString { .. }
        | PushTrue
        | PushFalse
        | PushNull
        | PushUndefined
        | PushNaN
        | PushNamespace { .. }
        | GetLocal { .. }
        | GetGlobalScope
        | GetScopeObject { .. }
        | GetOuterScope { .. }
        | NewFunction { .. } => (0, 1),
        GetLex { index } if rt(index) == 0 => (0, 1),
        FindProperty { index } | FindPropStrict { index } => (rt(index), 1),
        GetProperty { index } | GetSuper { index } => (plus(1, index)?, 1),
        GetSlot { .. }
        | Coerce { .. }
        | CoerceA
        | CoerceB
        | CoerceD
        | CoerceI
        | CoerceO
        | CoerceS
        | CoerceU
        | ConvertB
        | ConvertD
        | ConvertI
        | ConvertO
        | ConvertS
        | ConvertU
        | AsType { .. }
        | Negate
        | NegateI
        | Not
        | Increment
        | IncrementI
        | Decrement
        | DecrementI
        | TypeOf
        | BitNot
        | IsType { .. } => (1, 1),
        Add | AddI | Subtract | SubtractI | Multiply | MultiplyI | Divide | Modulo | Equals
        | StrictEquals | LessThan | LessEquals | GreaterThan | GreaterEquals | BitAnd | BitOr
        | BitXor | LShift | RShift | URShift | AsTypeLate | InstanceOf | In | IsTypeLate => (2, 1),
        CallProperty { index, num_args }
        | CallPropLex { index, num_args }
        | ConstructProp { index, num_args }
        | CallSuper { index, num_args } => (plus(num_args.checked_add(1)?, index)?, 1),
        CallPropVoid { index, num_args } | CallSuperVoid { index, num_args } => {
            (plus(num_args.checked_add(1)?, index)?, 0)
        }
        Call { num_args } => (num_args.checked_add(2)?, 1),
        CallMethod { num_args, .. } | CallStatic { num_args, .. } | Construct { num_args } => {
            (num_args.checked_add(1)?, 1)
        }
        NewArray { num_args } => (*num_args, 1),
        NewObject { num_args } => (num_args.checked_mul(2)?, 1),
        SetLocal { .. } | Pop => (1, 0),
        SetProperty { index } | InitProperty { index } => (plus(2, index)?, 0),
        Kill { .. }
        | Nop
        | DebugLine { .. }
        | DebugFile { .. }
        | Debug { .. }
        | IncLocal { .. }
        | IncLocalI { .. }
        | DecLocal { .. }
        | DecLocalI { .. }
        | BkptLine { .. } => (0, 0),
        _ => return None,
    })
}

/// The op that pushed the receiver of the call at `at`, which takes
/// `args` arguments above it, by straight-line look-back.
fn receiver<'o>(p: &Pool<'_>, ops: &'o [Op], at: usize, args: u32) -> Option<&'o Op> {
    let mut need = i64::from(args);
    for op in ops[..at].iter().rev() {
        let (pops, pushes) = effect(p, op)?;
        need -= i64::from(pushes);
        if need < 0 {
            return (need == -1 && pushes == 1).then_some(op);
        }
        need += i64::from(pops);
    }
    None
}

/// The first argument of the call at `at` when it is a constant string.
fn first_string(p: &Pool<'_>, ops: &[Op], at: usize, args: u32) -> Option<String> {
    if args == 0 {
        return None;
    }
    // The first argument is the op pushed just after the receiver: look
    // back for the op whose push leaves `args - 1` values above it.
    let mut need = i64::from(args) - 1;
    for op in ops[..at].iter().rev() {
        let (pops, pushes) = effect(p, op)?;
        need -= i64::from(pushes);
        if need < 0 {
            return match op {
                Op::PushString { value } if need == -1 => Some(p.string(*value)),
                _ => None,
            };
        }
        need += i64::from(pops);
    }
    None
}

impl Reading {
    /// Public instance methods of `class`, its own and those inherited
    /// from classes of the same block: (declaring class, member).
    pub fn public_methods(&self, class: &str) -> Vec<(&str, &Member)> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        let mut at = Some(class);
        while let Some(name) = at {
            let Some(c) = self.classes.iter().find(|c| c.name == name) else {
                break;
            };
            if !seen.insert(name) {
                break;
            }
            for m in &c.members {
                if m.access == Access::Public
                    && m.kind == MemberKind::Method
                    && !m.is_static
                    && out.iter().all(|(_, x): &(&str, &Member)| x.name != m.name)
                {
                    out.push((c.name.as_str(), m));
                }
            }
            at = Some(c.super_name.as_str());
        }
        out
    }
}

/// Reads one AVM2 block. `instance_names` are the names the movie's
/// placements give (members with those names are display children, not
/// host objects).
pub fn read(bytes: &[u8], file: &str, instance_names: &BTreeSet<String>) -> Result<Reading> {
    let abc = guarded(file, || Reader::new(bytes).read())?
        .map_err(|e| Error::new("BadAbc", file, 0, "readable ABC block", format!("{e:?}")))?;
    let p = Pool(&abc);
    let mut r = Reading::default();
    // Classes and the owner of each method.
    let mut owner: BTreeMap<u32, (String, String)> = BTreeMap::new();
    for (i, inst) in abc.instances.iter().enumerate() {
        let name = p.qualified(inst.name);
        let mut members: Vec<Member> = inst.traits.iter().map(|t| member(&p, t, false)).collect();
        if let Some(c) = abc.classes.get(i) {
            members.extend(c.traits.iter().map(|t| member(&p, t, true)));
            owner.insert(c.init_method.0, (name.clone(), "(static init)".into()));
            for t in &c.traits {
                if let TraitKind::Method { method, .. }
                | TraitKind::Getter { method, .. }
                | TraitKind::Setter { method, .. } = &t.kind
                {
                    owner.insert(method.0, (name.clone(), p.name(t.name).unwrap_or_default()));
                }
            }
        }
        owner.insert(inst.init_method.0, (name.clone(), "(constructor)".into()));
        for t in &inst.traits {
            if let TraitKind::Method { method, .. }
            | TraitKind::Getter { method, .. }
            | TraitKind::Setter { method, .. } = &t.kind
            {
                owner.insert(method.0, (name.clone(), p.name(t.name).unwrap_or_default()));
            }
        }
        r.classes.push(Class {
            name,
            super_name: p.qualified(inst.super_name),
            members,
        });
    }
    // Decode every body.
    let mut bodies: Vec<(u32, u32, Vec<Op>)> = Vec::new();
    for b in &abc.method_bodies {
        r.method_bodies += 1;
        let mut ops = Vec::new();
        let mut rd = Reader::new(&b.code);
        let mut refused = false;
        while !rd.as_slice().is_empty() {
            match guarded(file, || rd.read_op())? {
                Ok(op) => ops.push(op),
                Err(_) => {
                    refused = true;
                    break;
                }
            }
        }
        r.refused_bodies += u64::from(refused);
        let params = abc
            .methods
            .get(b.method.0 as usize)
            .map_or(0, |m| m.params.len() as u32);
        bodies.push((b.method.0, params, ops));
    }
    // Names to which the movie's own code gives a value. A store of an
    // empty object (`{}` or `new Object()`), null or undefined is a
    // placeholder; a store of the writing method's parameter hands on a
    // value from its caller (the reading does not follow it further).
    let mut written: BTreeSet<String> = BTreeSet::new();
    for (_, params, ops) in &bodies {
        for (k, op) in ops.iter().enumerate() {
            if let Op::SetProperty { index } | Op::InitProperty { index } = op {
                let handed = k > 0
                    && match &ops[k - 1] {
                        Op::NewObject { num_args: 0 } | Op::PushNull | Op::PushUndefined => true,
                        Op::ConstructProp { index, num_args: 0 } => {
                            p.name(*index).as_deref() == Some("Object")
                        }
                        Op::GetLocal { index } => (1..=*params).contains(index),
                        _ => false,
                    };
                if let (false, Some(n)) = (handed, p.name(*index)) {
                    written.insert(n);
                }
            }
        }
    }
    let mut candidates: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for c in &r.classes {
        for m in &c.members {
            if matches!(m.kind, MemberKind::Slot)
                && !m.is_static
                && matches!(m.type_name.as_str(), "*" | "Object")
                && !written.contains(&m.name)
                && !instance_names.contains(&m.name)
            {
                let e = candidates
                    .entry(m.name.clone())
                    .or_insert_with(|| (m.type_name.clone(), Vec::new()));
                e.1.push(c.name.clone());
            }
        }
    }
    for (method, _, ops) in &bodies {
        let (class, mname) = owner
            .get(method)
            .cloned()
            .unwrap_or_else(|| ("(script or closure)".into(), String::new()));
        for (at, op) in ops.iter().enumerate() {
            let (index, args) = match op {
                Op::CallProperty { index, num_args }
                | Op::CallPropVoid { index, num_args }
                | Op::CallPropLex { index, num_args } => (*index, *num_args),
                _ => continue,
            };
            r.call_sites += 1;
            let Some(called) = p.name(index) else {
                r.unresolved_calls += 1;
                continue;
            };
            let Some(recv) = receiver(&p, ops, at, args) else {
                r.unresolved_calls += 1;
                continue;
            };
            let recv_name = match recv {
                Op::GetLex { index } | Op::GetProperty { index } => p.name(*index),
                Op::FindPropStrict { index } | Op::FindProperty { index } => {
                    p.name(*index).map(|n| format!("(scope){n}"))
                }
                _ => None,
            };
            let via = match recv_name.as_deref() {
                Some("ExternalInterface") if called == "call" => Via::External,
                Some("(scope)fscommand") if called == "fscommand" => Via::Command,
                Some(n) if candidates.contains_key(n) => Via::Object(n.to_string()),
                _ => continue,
            };
            let name = match via {
                Via::External | Via::Command => match first_string(&p, ops, at, args) {
                    Some(s) => s,
                    None => {
                        r.unresolved_calls += 1;
                        continue;
                    }
                },
                Via::Object(_) => called.clone(),
            };
            let args = match via {
                Via::External | Via::Command => args.saturating_sub(1),
                Via::Object(_) => args,
            };
            r.host_calls.push(HostCall {
                via,
                name,
                args,
                class: class.clone(),
                method: mname.clone(),
            });
        }
    }
    r.host_calls.sort();
    r.host_calls.dedup();
    // A host object is a candidate the movie calls through.
    for (name, (type_name, classes)) in candidates {
        if r.host_calls
            .iter()
            .any(|h| matches!(&h.via, Via::Object(o) if *o == name))
        {
            r.host_objects.push((name, type_name));
            for c in classes {
                if !r.host_classes.contains(&c) {
                    r.host_classes.push(c);
                }
            }
        }
    }
    Ok(r)
}

/// A text listing of the ops of every method of the classes whose name
/// contains `filter` (notes only; never versioned): one op per line, with
/// the names of the multinames and strings it takes.
pub fn listing(bytes: &[u8], file: &str, filter: &str) -> Result<String> {
    use std::fmt::Write as _;
    let abc = guarded(file, || Reader::new(bytes).read())?
        .map_err(|e| Error::new("BadAbc", file, 0, "readable ABC block", format!("{e:?}")))?;
    let p = Pool(&abc);
    let mut methods: Vec<(u32, String)> = Vec::new();
    for (i, inst) in abc.instances.iter().enumerate() {
        let name = p.qualified(inst.name);
        if !name.contains(filter) {
            continue;
        }
        methods.push((inst.init_method.0, format!("{name}.(constructor)")));
        let statics = abc
            .classes
            .get(i)
            .map(|c| c.traits.as_slice())
            .unwrap_or(&[]);
        for t in inst.traits.iter().chain(statics) {
            if let TraitKind::Method { method, .. }
            | TraitKind::Getter { method, .. }
            | TraitKind::Setter { method, .. } = &t.kind
            {
                methods.push((
                    method.0,
                    format!("{name}.{}", p.name(t.name).unwrap_or_default()),
                ));
            }
        }
    }
    let mut out = String::new();
    for (m, label) in methods {
        let Some(body) = abc
            .methods
            .get(m as usize)
            .and_then(|x| x.body)
            .and_then(|b| abc.method_bodies.get(b.0 as usize))
        else {
            continue;
        };
        let _ = writeln!(out, "== {label}");
        let mut rd = Reader::new(&body.code);
        while !rd.as_slice().is_empty() {
            let Ok(Ok(op)) = guarded(file, || rd.read_op()) else {
                let _ = writeln!(out, "   (refused)");
                break;
            };
            let extra = match &op {
                Op::GetLex { index }
                | Op::GetProperty { index }
                | Op::SetProperty { index }
                | Op::InitProperty { index }
                | Op::FindPropStrict { index }
                | Op::FindProperty { index }
                | Op::CallProperty { index, .. }
                | Op::CallPropVoid { index, .. }
                | Op::CallPropLex { index, .. }
                | Op::ConstructProp { index, .. }
                | Op::Coerce { index }
                | Op::IsType { index } => p.qualified(*index),
                Op::PushString { value } => format!("{:?}", p.string(*value)),
                _ => String::new(),
            };
            let _ = writeln!(out, "   {op:?} {extra}");
        }
    }
    Ok(out)
}

/// How a method's code names a member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UseKind {
    /// Read (`getproperty`, `getlex`).
    Read,
    /// Written (`setproperty`, `initproperty`).
    Write,
    /// Called (`callproperty` and its kin).
    Call,
    /// Its `text` or `htmlText` written (the receiver of the store named by
    /// the straight-line look-back).
    TextWrite,
}

/// One place where code names a member.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Use {
    pub name: String,
    pub kind: UseKind,
    pub class: String,
    pub method: String,
}

/// Every place the block's code names a member by a static name.
pub fn uses(bytes: &[u8], file: &str) -> Result<Vec<Use>> {
    let abc = guarded(file, || Reader::new(bytes).read())?
        .map_err(|e| Error::new("BadAbc", file, 0, "readable ABC block", format!("{e:?}")))?;
    let p = Pool(&abc);
    let mut owner: BTreeMap<u32, (String, String)> = BTreeMap::new();
    for (i, inst) in abc.instances.iter().enumerate() {
        let name = p.qualified(inst.name);
        owner.insert(inst.init_method.0, (name.clone(), "(constructor)".into()));
        let statics = abc
            .classes
            .get(i)
            .map(|c| c.traits.as_slice())
            .unwrap_or(&[]);
        if let Some(c) = abc.classes.get(i) {
            owner.insert(c.init_method.0, (name.clone(), "(static init)".into()));
        }
        for t in inst.traits.iter().chain(statics) {
            if let TraitKind::Method { method, .. }
            | TraitKind::Getter { method, .. }
            | TraitKind::Setter { method, .. } = &t.kind
            {
                owner.insert(method.0, (name.clone(), p.name(t.name).unwrap_or_default()));
            }
        }
    }
    let mut out = Vec::new();
    for b in &abc.method_bodies {
        let (class, method) = owner
            .get(&b.method.0)
            .cloned()
            .unwrap_or_else(|| ("(script or closure)".into(), String::new()));
        let mut ops = Vec::new();
        let mut rd = Reader::new(&b.code);
        while !rd.as_slice().is_empty() {
            match guarded(file, || rd.read_op())? {
                Ok(op) => ops.push(op),
                Err(_) => break,
            }
        }
        for (at, op) in ops.iter().enumerate() {
            let (index, kind) = match op {
                Op::GetProperty { index } | Op::GetLex { index } => (*index, UseKind::Read),
                Op::SetProperty { index } | Op::InitProperty { index } => (*index, UseKind::Write),
                Op::CallProperty { index, .. }
                | Op::CallPropVoid { index, .. }
                | Op::CallPropLex { index, .. } => (*index, UseKind::Call),
                _ => continue,
            };
            let Some(name) = p.name(index) else { continue };
            if kind == UseKind::Write && matches!(name.as_str(), "text" | "htmlText") {
                if let Some(Op::GetProperty { index: r } | Op::GetLex { index: r }) =
                    receiver(&p, &ops, at, 1)
                {
                    if let Some(field) = p.name(*r) {
                        out.push(Use {
                            name: field,
                            kind: UseKind::TextWrite,
                            class: class.clone(),
                            method: method.clone(),
                        });
                    }
                }
            }
            out.push(Use {
                name,
                kind,
                class: class.clone(),
                method: method.clone(),
            });
        }
    }
    out.sort();
    Ok(out)
}
