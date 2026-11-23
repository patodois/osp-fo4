//! Running instructions under the candidate meanings (`spec/vm.md`,
//! Opcodes). Each opcode records its rule ([`crate::rules::of_opcode`]).
use crate::{
    host::{NativeCall, Reply},
    program::{Func, Ins, Lit},
    rules,
    world::{InstId, ObjId, Origin, StructVal, Val},
    Block, Effect, Site, Stop, Vm, VmError, R,
};
use std::rc::Rc;

pub(crate) struct Frame {
    func: Rc<Func>,
    inst: Option<InstId>,
    /// Parameters, then locals.
    slots: Vec<Val>,
    /// Whether each local was written (reading one that was not relies on
    /// `pex_default_values_v1`).
    written: Vec<bool>,
    pc: usize,
}

/// What an identifier names in a frame.
enum Place {
    Slot(usize),
    SelfRef,
    State,
    Var(InstId, usize),
}

/// A truth value or an undetermined one.
enum Truth {
    Known(bool),
    Unknown(usize),
    /// An array without elements: `pex_truth_of_values_v1` leaves it open.
    EmptyArray,
}

fn slot(a: &[Lit], i: usize) -> R<&Lit> {
    a.get(i).ok_or_else(|| {
        Stop::err(VmError::BadArgument {
            site: None,
            detail: format!("missing slot {i}"),
        })
    })
}

fn kind_text(v: &Val) -> &'static str {
    match v {
        Val::None => "none",
        Val::Int(_) => "int",
        Val::Float(_) => "float",
        Val::Bool(_) => "bool",
        Val::Str(_) => "string",
        Val::Object(_) => "object",
        Val::Array(_) => "array",
        Val::Struct(_) => "struct",
        Val::Unknown(_) => "undetermined",
    }
}

impl Vm<'_> {
    pub(crate) fn kind_of(&self, v: &Val) -> String {
        match v {
            Val::Array(a) => format!("{}[]", self.world.arrays[*a].elem),
            Val::Struct(s) => self.world.structs[*s].type_name.clone(),
            _ => kind_text(v).to_string(),
        }
    }
    /// Whether a value may be stored in a slot of a declared type.
    pub(crate) fn fits(&mut self, v: &Val, ty: &str) -> Result<bool, VmError> {
        Ok(match (ty, v) {
            ("var", _) | (_, Val::Unknown(_)) => true,
            ("int", x) => matches!(x, Val::Int(_)),
            ("float", x) => matches!(x, Val::Float(_)),
            ("bool", x) => matches!(x, Val::Bool(_)),
            ("string", x) => matches!(x, Val::Str(_)),
            ("none", x) => matches!(x, Val::None),
            (t, Val::None) => !matches!(t, "int" | "float" | "bool" | "string"),
            (t, Val::Array(a)) => t.strip_suffix("[]") == Some(self.world.arrays[*a].elem.as_str()),
            (t, Val::Struct(s)) => self.world.structs[*s].type_name == t,
            (t, Val::Object(_)) => !t.ends_with("[]") && !t.contains('#'),
            _ => false,
        })
    }
    fn site(f: &Frame) -> Site {
        Site {
            script: f.func.script.clone(),
            state: f.func.state.clone(),
            function: f.func.name.clone(),
            index: f.pc,
            op: f.func.code.get(f.pc).map_or(0, |i| i.op),
        }
    }
    fn block(&self, u: usize, f: &Frame) -> Stop {
        Stop::blocked(Block {
            on: self.world.origin(u).clone(),
            at: Some(Self::site(f)),
        })
    }
    fn mismatch(f: &Frame, expected: impl Into<String>, found: impl Into<String>) -> Stop {
        Stop::err(VmError::TypeMismatch {
            site: Some(Box::new(Self::site(f))),
            expected: expected.into(),
            found: found.into(),
        })
    }
    fn bad(f: &Frame, detail: impl Into<String>) -> Stop {
        Stop::err(VmError::BadArgument {
            site: Some(Box::new(Self::site(f))),
            detail: detail.into(),
        })
    }
    fn unknown(&mut self, origin: Origin) -> R<Val> {
        if self.world.unknowns.len() >= self.limits.values {
            return Err(Stop::err(VmError::BadArgument {
                site: None,
                detail: "too many undetermined values".into(),
            }));
        }
        Ok(self.world.unknown(origin))
    }

    // ---- names -------------------------------------------------------

    fn place(&mut self, f: &Frame, name: &str) -> R<Place> {
        if let Some(i) = f
            .func
            .params
            .iter()
            .chain(&f.func.locals)
            .position(|(n, _)| n == name)
        {
            return Ok(Place::Slot(i));
        }
        if name == "self" {
            return Ok(Place::SelfRef);
        }
        if name == "::state" {
            return Ok(Place::State);
        }
        if let Some(inst) = f.inst {
            for s in self.chain(&f.func.script)? {
                if s.variable(name).is_some() {
                    if let Some(i) = self.world.instances[inst]
                        .vars
                        .iter()
                        .position(|v| v.script == s.name && v.name == name)
                    {
                        return Ok(Place::Var(inst, i));
                    }
                }
            }
        }
        Err(Stop::err(VmError::UnknownName {
            site: Box::new(Self::site(f)),
            name: name.to_string(),
        }))
    }
    /// Declared type of a slot (literals: their kind).
    fn decl_type(&mut self, f: &Frame, l: &Lit) -> R<String> {
        Ok(match l {
            Lit::Ident(n) => match self.place(f, n)? {
                Place::Slot(i) => f
                    .func
                    .params
                    .iter()
                    .chain(&f.func.locals)
                    .nth(i)
                    .map(|x| x.1.clone())
                    .unwrap_or_default(),
                Place::SelfRef => f
                    .inst
                    .map(|i| self.world.instances[i].script.clone())
                    .unwrap_or_else(|| f.func.script.clone()),
                Place::State => "string".into(),
                Place::Var(i, k) => self.world.instances[i].vars[k].type_name.clone(),
            },
            Lit::None => "none".into(),
            Lit::Str(_) => "string".into(),
            Lit::Int(_) => "int".into(),
            Lit::Float(_) => "float".into(),
            Lit::Bool(_) => "bool".into(),
        })
    }
    fn read(&mut self, f: &mut Frame, l: &Lit) -> R<Val> {
        Ok(match l {
            Lit::None => Val::None,
            Lit::Int(n) => Val::Int(*n),
            Lit::Float(x) => Val::Float(*x),
            Lit::Bool(b) => Val::Bool(*b),
            Lit::Str(s) => Val::Str(s.clone()),
            Lit::Ident(n) => match self.place(f, n)? {
                Place::Slot(i) => {
                    let np = f.func.params.len();
                    if i >= np && !f.written[i - np] {
                        self.rules.insert(rules::DEFAULTS);
                    }
                    f.slots[i].clone()
                }
                Place::SelfRef => match f.inst.and_then(|i| self.world.instances[i].object) {
                    Some(o) => Val::Object(o),
                    None => return Err(Self::bad(f, "self without an object")),
                },
                Place::State => {
                    self.rules.insert(rules::STATE);
                    match f.inst {
                        Some(i) => Val::Str(self.world.instances[i].state.clone()),
                        None => return Err(Self::bad(f, "::State without an instance")),
                    }
                }
                Place::Var(i, k) => self.world.instances[i].vars[k].value.clone(),
            },
        })
    }
    fn write(&mut self, f: &mut Frame, l: &Lit, v: Val) -> R<()> {
        let Lit::Ident(n) = l else {
            return Err(Self::bad(f, format!("destination is a literal {l:?}")));
        };
        let ty = self.decl_type(f, l)?;
        if !self.fits(&v, &ty)? {
            let found = self.kind_of(&v);
            return Err(Self::mismatch(f, ty, found));
        }
        match self.place(f, n)? {
            Place::Slot(i) => {
                let np = f.func.params.len();
                if i >= np {
                    f.written[i - np] = true;
                }
                f.slots[i] = v;
            }
            Place::SelfRef => return Err(Self::bad(f, "write to self")),
            Place::State => {
                self.rules.insert(rules::STATE);
                let Val::Str(s) = v else {
                    return Err(Self::mismatch(f, "string", kind_text(&v)));
                };
                match f.inst {
                    Some(i) => self.world.instances[i].state = s.to_ascii_lowercase(),
                    None => return Err(Self::bad(f, "::State without an instance")),
                }
            }
            Place::Var(i, k) => {
                self.world.instances[i].vars[k].value = v;
                self.variable_written(i, k);
            }
        }
        Ok(())
    }
    /// Keeps a write of an instance's variable for the callers that offer
    /// waiting scenes again on it (`scene::Scenes::variable_writes`, only
    /// while they keep them).
    fn variable_written(&mut self, i: InstId, k: usize) {
        if self.world.scenes.variable_writes.is_none() {
            return;
        }
        let Some(o) = self.world.instances[i].object else {
            return;
        };
        let name = self.world.instances[i].vars[k].name.clone();
        if let Some(set) = self.world.scenes.variable_writes.as_mut() {
            set.insert((o, name));
        }
    }
    fn ident<'l>(f: &Frame, l: &'l Lit) -> R<&'l str> {
        match l {
            Lit::Ident(n) => Ok(n),
            _ => Err(Self::bad(f, format!("expected a name, found {l:?}"))),
        }
    }
    fn int_arg(&mut self, f: &mut Frame, l: &Lit) -> R<i32> {
        match self.read(f, l)? {
            Val::Int(n) => Ok(n),
            Val::Unknown(u) => Err(self.block(u, f)),
            v => Err(Self::mismatch(f, "int", kind_text(&v))),
        }
    }

    // ---- calls -------------------------------------------------------

    /// The function a call of `name` on an instance runs: the nearest
    /// script of the instance's chain declaring it in the current state,
    /// else in the empty state (`pex_call_dispatch_v1`).
    pub(crate) fn dispatch(&mut self, inst: InstId, name: &str) -> R<Option<Rc<Func>>> {
        let script = self.world.instances[inst].script.clone();
        let state = self.world.instances[inst].state.clone();
        self.find_from(&script, &state, name)
    }
    fn find_from(&mut self, script: &str, state: &str, name: &str) -> R<Option<Rc<Func>>> {
        let chain = self.chain(script)?;
        if !state.is_empty() {
            if let Some(f) = chain.iter().find_map(|s| s.function(state, name)) {
                self.rules.insert(rules::STATE);
                return Ok(Some(f.clone()));
            }
        }
        Ok(chain.iter().find_map(|s| s.function("", name)).cloned())
    }
    /// Static resolution from a type: the nearest script declaring the
    /// name, its empty-state function first.
    fn resolve(&mut self, type_name: &str, name: &str) -> R<Option<Rc<Func>>> {
        for s in self.chain(type_name)? {
            if let Some(f) = s.function("", name) {
                return Ok(Some(f.clone()));
            }
            if let Some(f) = s.functions.iter().find(|(k, _)| k.1 == name).map(|x| x.1) {
                return Ok(Some(f.clone()));
            }
        }
        Ok(None)
    }
    /// An instance of an object whose script is or extends `type_name`.
    fn instance_of(&mut self, o: ObjId, type_name: &str) -> R<Vec<InstId>> {
        self.attach_scripts(o)?;
        let mut out = Vec::new();
        for i in self.world.objects[o].instances.clone() {
            let s = self.world.instances[i].script.clone();
            if self.is_a(&s, type_name)? {
                out.push(i);
            }
        }
        Ok(out)
    }
    /// An instance without variables of a script on an object, for a
    /// scripted function declared by a script with no variables in its
    /// chain; `None` when the chain declares variables.
    fn implicit_instance(&mut self, o: ObjId, script: &str) -> R<Option<InstId>> {
        if self.chain(script)?.iter().any(|s| !s.variables.is_empty()) {
            return Ok(None);
        }
        for i in self.world.objects[o].instances.clone() {
            if self.world.instances[i].script == script {
                return Ok(Some(i));
            }
        }
        Ok(Some(self.instantiate(Some(o), script)?))
    }

    #[inline(never)]
    pub(crate) fn call(&mut self, inst: Option<InstId>, func: Rc<Func>, args: Vec<Val>) -> R<Val> {
        if self.depth >= self.limits.depth {
            return Err(Stop::err(VmError::DepthLimit(self.limits.depth)));
        }
        if func.native() {
            return Err(Stop::err(VmError::BadArgument {
                site: None,
                detail: format!("{}.{} is native", func.script, func.name),
            }));
        }
        if args.len() != func.params.len() {
            return Err(Stop::err(VmError::BadArgument {
                site: None,
                detail: format!(
                    "{}.{} takes {} arguments, given {}",
                    func.script,
                    func.name,
                    func.params.len(),
                    args.len()
                ),
            }));
        }
        for (a, (_, t)) in args.iter().zip(&func.params) {
            if !self.fits(a, t)? {
                return Err(Stop::err(VmError::TypeMismatch {
                    site: None,
                    expected: t.clone(),
                    found: self.kind_of(a),
                }));
            }
        }
        let mut slots = args;
        for (_, t) in &func.locals {
            let v = match t.as_str() {
                "int" => Val::Int(0),
                "float" => Val::Float(0.0),
                "bool" => Val::Bool(false),
                "string" => Val::Str(String::new()),
                _ => Val::None,
            };
            slots.push(v);
        }
        let mut f = Frame {
            written: vec![false; func.locals.len()],
            func,
            inst,
            slots,
            pc: 0,
        };
        self.depth += 1;
        let r = self.body(&mut f);
        self.depth -= 1;
        let v = r?;
        let rt = f.func.return_type.clone();
        if !self.fits(&v, &rt)? && !(rt == "none" && matches!(v, Val::None)) {
            let found = self.kind_of(&v);
            return Err(Self::mismatch(&f, rt, found));
        }
        Ok(v)
    }
    fn body(&mut self, f: &mut Frame) -> R<Val> {
        let func = f.func.clone();
        loop {
            if f.pc == func.code.len() {
                // Past the last instruction: the return type's default
                // (`pex_op_return_v1`, `pex_default_values_v1`).
                let rt = func.return_type.clone();
                return Ok(self.default_of(&rt));
            }
            let Some(ins) = func.code.get(f.pc) else {
                return Err(Self::bad(f, "instruction index past the end"));
            };
            self.steps += 1;
            if self.steps > self.limits.steps {
                return Err(Stop::err(VmError::StepLimit(self.limits.steps)));
            }
            let Some(rule) = rules::of_opcode(ins.op) else {
                return Err(Stop::err(VmError::UnmeasuredOpcode {
                    site: Box::new(Self::site(f)),
                }));
            };
            self.rules.insert(rule);
            if let Some(v) = self.step(f, ins)? {
                return Ok(v);
            }
        }
    }
    #[inline(never)]
    fn native(
        &mut self,
        f: &Frame,
        func: &Func,
        receiver: Option<Val>,
        instance: Option<InstId>,
        args: Vec<Val>,
    ) -> R<Val> {
        let call = NativeCall {
            script: func.script.clone(),
            name: func.name.clone(),
            receiver: receiver.clone(),
            instance,
            args,
            param_types: func.params.iter().map(|p| p.1.clone()).collect(),
            return_type: func.return_type.clone(),
        };
        let reply = self.host.call(&call, &mut self.world);
        for r in self.host.rules() {
            self.rules.insert(r);
        }
        let full = call.full_name();
        let v = match reply {
            Reply::Value(v) => v,
            Reply::Undetermined => self.unknown(Origin::Native(full.clone()))?,
            Reply::NotProvided => return Err(Stop::err(VmError::NativeNotProvided(full))),
        };
        let effect = Effect {
            depth: self.depth.saturating_sub(1),
            caller: format!("{}.{}", f.func.script, f.func.name),
            native: full,
            receiver: receiver.as_ref().map(|r| self.world.render(r)),
            receiver_object: match receiver {
                Some(Val::Object(o)) => Some(o),
                _ => None,
            },
            args: call.args.iter().map(|a| self.world.render(a)).collect(),
            result: self.world.render(&v),
        };
        self.effects.push(effect);
        if !self.world.quests.queue.is_empty() {
            if let Some(n) = self.nested.take() {
                n.run(self);
                self.nested = Some(n);
            }
        }
        Ok(v)
    }
    fn call_values(&mut self, f: &mut Frame, ins: &Ins) -> R<Vec<Val>> {
        let mut v = Vec::with_capacity(ins.extra.len());
        for l in &ins.extra {
            v.push(self.read(f, l)?);
        }
        Ok(v)
    }
    #[inline(never)]
    fn op_call_method(&mut self, f: &mut Frame, ins: &Ins) -> R<()> {
        let [name, recv, dest, ..] = ins.args.as_slice() else {
            return Err(Self::bad(f, "call slots"));
        };
        let name = Self::ident(f, name)?.to_string();
        let args = self.call_values(f, ins)?;
        let is_self = matches!(recv, Lit::Ident(n) if n == "self");
        let rv = self.read(f, recv)?;
        let rtype = self.decl_type(f, recv)?;
        let v = match rv {
            Val::None => {
                return Err(Stop::err(VmError::NoneOperand {
                    site: Box::new(Self::site(f)),
                    what: format!("call of {name}"),
                }))
            }
            Val::Unknown(u) => {
                let func = self.resolve(&rtype, &name)?.ok_or_else(|| {
                    Stop::err(VmError::MissingFunction {
                        script: rtype.clone(),
                        name: name.clone(),
                    })
                })?;
                if !func.native() {
                    return Err(self.block(u, f));
                }
                self.native(f, &func, Some(rv), None, args)?
            }
            Val::Object(o) => {
                let insts = if is_self {
                    f.inst.into_iter().collect()
                } else {
                    self.instance_of(o, &rtype)?
                };
                let mut chosen: Option<(InstId, Rc<Func>)> = None;
                for i in &insts {
                    if let Some(func) = self.dispatch(*i, &name)? {
                        match &chosen {
                            Some((_, g)) if !Rc::ptr_eq(g, &func) => {
                                return Err(Stop::err(VmError::Ambiguous {
                                    site: Box::new(Self::site(f)),
                                    detail: format!("{name} on two script instances"),
                                }))
                            }
                            Some(_) => {}
                            None => chosen = Some((*i, func)),
                        }
                    }
                }
                let (inst, func) = match chosen {
                    Some((i, func)) => (Some(i), func),
                    None => {
                        let func = self.resolve(&rtype, &name)?.ok_or_else(|| {
                            Stop::err(VmError::MissingFunction {
                                script: rtype.clone(),
                                name: name.clone(),
                            })
                        })?;
                        (None, func)
                    }
                };
                if func.native() {
                    let inst = inst.or_else(|| insts.first().copied());
                    self.native(f, &func, Some(rv), inst, args)?
                } else {
                    let inst = match inst {
                        Some(i) => i,
                        None => match self.implicit_instance(o, &func.script)? {
                            Some(i) => i,
                            None => {
                                return Err(Stop::blocked(Block {
                                    on: Origin::NoInstance(rtype, self.world.object_kind(o)),
                                    at: Some(Self::site(f)),
                                }))
                            }
                        },
                    };
                    self.call(Some(inst), func, args)?
                }
            }
            other => return Err(Self::mismatch(f, rtype, kind_text(&other))),
        };
        self.write(f, dest, v)
    }
    #[inline(never)]
    fn op_call_parent(&mut self, f: &mut Frame, ins: &Ins) -> R<()> {
        let [name, dest, ..] = ins.args.as_slice() else {
            return Err(Self::bad(f, "call slots"));
        };
        let name = Self::ident(f, name)?.to_string();
        let args = self.call_values(f, ins)?;
        let Some(inst) = f.inst else {
            return Err(Self::bad(f, "parent call without an instance"));
        };
        let parent = self.script(&f.func.script)?.parent.clone();
        let state = self.world.instances[inst].state.clone();
        let func = self.find_from(&parent, &state, &name)?.ok_or_else(|| {
            Stop::err(VmError::MissingFunction {
                script: parent.clone(),
                name: name.clone(),
            })
        })?;
        let v = if func.native() {
            let recv = self.world.instances[inst].object.map(Val::Object);
            self.native(f, &func, recv, Some(inst), args)?
        } else {
            self.call(Some(inst), func, args)?
        };
        self.write(f, dest, v)
    }
    #[inline(never)]
    fn op_call_static(&mut self, f: &mut Frame, ins: &Ins) -> R<()> {
        let [script, name, dest, ..] = ins.args.as_slice() else {
            return Err(Self::bad(f, "call slots"));
        };
        let script = Self::ident(f, script)?.to_string();
        let name = Self::ident(f, name)?.to_string();
        let args = self.call_values(f, ins)?;
        let func = self.resolve(&script, &name)?.ok_or_else(|| {
            Stop::err(VmError::MissingFunction {
                script: script.clone(),
                name: name.clone(),
            })
        })?;
        let v = if func.native() {
            self.native(f, &func, None, None, args)?
        } else {
            self.call(None, func, args)?
        };
        self.write(f, dest, v)
    }

    // ---- properties ------------------------------------------------------

    #[inline(never)]
    fn property_instance(&mut self, f: &mut Frame, recv: &Lit, name: &str) -> R<InstId> {
        if matches!(recv, Lit::Ident(n) if n == "self") {
            return f
                .inst
                .ok_or_else(|| Self::bad(f, "self without an instance"));
        }
        let rtype = self.decl_type(f, recv)?;
        match self.read(f, recv)? {
            Val::Object(o) => {
                let insts = self.instance_of(o, &rtype)?;
                match insts.as_slice() {
                    [i] => Ok(*i),
                    [] => Err(Stop::blocked(Block {
                        on: Origin::NoInstance(rtype, self.world.object_kind(o)),
                        at: Some(Self::site(f)),
                    })),
                    _ => Err(Stop::err(VmError::Ambiguous {
                        site: Box::new(Self::site(f)),
                        detail: format!("property {name} on two script instances"),
                    })),
                }
            }
            Val::Unknown(u) => Err(self.block(u, f)),
            Val::None => Err(Stop::err(VmError::NoneOperand {
                site: Box::new(Self::site(f)),
                what: format!("property {name}"),
            })),
            v => Err(Self::mismatch(f, rtype, kind_text(&v))),
        }
    }
    #[inline(never)]
    fn property_access(
        &mut self,
        f: &mut Frame,
        inst: InstId,
        name: &str,
        set: Option<Val>,
    ) -> R<Val> {
        let script = self.world.instances[inst].script.clone();
        for s in self.chain(&script)? {
            let Some(p) = s.property(name) else { continue };
            if let Some(var) = &p.variable {
                let k = self.world.instances[inst]
                    .vars
                    .iter()
                    .position(|x| x.script == s.name && x.name == *var)
                    .ok_or_else(|| Self::bad(f, format!("variable {var} of property {name}")))?;
                return Ok(match set {
                    None => self.world.instances[inst].vars[k].value.clone(),
                    Some(v) => {
                        let ty = self.world.instances[inst].vars[k].type_name.clone();
                        if !self.fits(&v, &ty)? {
                            let found = self.kind_of(&v);
                            return Err(Self::mismatch(f, ty, found));
                        }
                        self.world.instances[inst].vars[k].value = v;
                        self.variable_written(inst, k);
                        Val::None
                    }
                });
            }
            let func = match &set {
                None => p.read.clone(),
                Some(_) => p.write.clone(),
            };
            let func = func.ok_or_else(|| {
                Self::bad(
                    f,
                    format!("property {name} has no function for this access"),
                )
            })?;
            return self.call(Some(inst), func, set.into_iter().collect());
        }
        Err(Stop::err(VmError::MissingProperty {
            script,
            name: name.to_string(),
        }))
    }

    // ---- values ------------------------------------------------------

    fn truth(&mut self, v: &Val) -> Truth {
        if !matches!(v, Val::Bool(_) | Val::Unknown(_)) {
            self.rules.insert(rules::TRUTH);
        }
        match v {
            Val::Bool(b) => Truth::Known(*b),
            Val::Int(n) => Truth::Known(*n != 0),
            Val::Float(x) => Truth::Known(*x != 0.0),
            Val::Str(s) => Truth::Known(!s.is_empty()),
            Val::None => Truth::Known(false),
            Val::Object(_) | Val::Struct(_) => Truth::Known(true),
            Val::Array(a) => {
                if self.world.arrays[*a].items.is_empty() {
                    Truth::EmptyArray
                } else {
                    Truth::Known(true)
                }
            }
            Val::Unknown(u) => Truth::Unknown(*u),
        }
    }
    fn truth_val(&mut self, v: &Val) -> R<Val> {
        Ok(match self.truth(v) {
            Truth::Known(b) => Val::Bool(b),
            Truth::EmptyArray => {
                self.unknown(Origin::Rule(rules::TRUTH, "truth of an empty array".into()))?
            }
            Truth::Unknown(u) => Val::Unknown(u),
        })
    }
    #[inline(never)]
    fn equal(&mut self, a: &Val, b: &Val) -> R<Val> {
        let known = |x: bool| Ok(Val::Bool(x));
        match (a, b) {
            (Val::Unknown(u), _) | (_, Val::Unknown(u)) => Ok(Val::Unknown(*u)),
            (Val::Int(x), Val::Int(y)) => known(x == y),
            (Val::Float(x), Val::Float(y)) => known(x == y),
            (Val::Int(x), Val::Float(y)) | (Val::Float(y), Val::Int(x)) => {
                known(f64::from(*x) == f64::from(*y))
            }
            (Val::Bool(x), Val::Bool(y)) => known(x == y),
            (Val::Str(x), Val::Str(y)) => {
                if x == y {
                    known(true)
                } else if x.eq_ignore_ascii_case(y) {
                    if self.texts_ignore_case {
                        self.rules.insert(rules::TEXT_EQUAL_IGNORE_CASE);
                        return known(true);
                    }
                    self.unknown(Origin::Rule(
                        rules::COMPARE,
                        "equality of texts that differ only in case".into(),
                    ))
                } else {
                    known(false)
                }
            }
            (Val::None, Val::None) => known(true),
            (Val::None, Val::Object(_) | Val::Array(_) | Val::Struct(_))
            | (Val::Object(_) | Val::Array(_) | Val::Struct(_), Val::None) => known(false),
            (Val::Object(x), Val::Object(y)) => {
                if x == y {
                    return known(true);
                }
                self.rules.insert(rules::IDENTITY);
                let (kx, ky) = (&self.world.objects[*x].key, &self.world.objects[*y].key);
                let player = self.world.player;
                if kx.is_some() && ky.is_some() {
                    known(false)
                } else if self.world.player_no_record
                    && ((player == Some(*x) && ky.is_some())
                        || (player == Some(*y) && kx.is_some()))
                {
                    // The player is no record's handle
                    // (`vm_player_no_record_v1`).
                    self.rules.insert(rules::PLAYER_NO_RECORD);
                    known(false)
                } else {
                    let d = format!(
                        "whether <{}> is <{}>",
                        self.world.objects[*x].label, self.world.objects[*y].label
                    );
                    self.unknown(Origin::Rule(rules::IDENTITY, d))
                }
            }
            (Val::Array(x), Val::Array(y)) => known(x == y),
            (Val::Struct(x), Val::Struct(y)) => known(x == y),
            (x, y) => self.unknown(Origin::Rule(
                rules::COMPARE,
                format!("equality of {} and {}", kind_text(x), kind_text(y)),
            )),
        }
    }
    fn order(&mut self, f: &Frame, op: u8, a: &Val, b: &Val) -> R<Val> {
        let (x, y) = match (a, b) {
            (Val::Unknown(u), _) | (_, Val::Unknown(u)) => return Ok(Val::Unknown(*u)),
            (Val::Int(x), Val::Int(y)) => (f64::from(*x), f64::from(*y)),
            (Val::Float(x), Val::Float(y)) => (f64::from(*x), f64::from(*y)),
            (Val::Int(x), Val::Float(y)) => (f64::from(*x), f64::from(*y)),
            (Val::Float(x), Val::Int(y)) => (f64::from(*x), f64::from(*y)),
            (Val::Str(_), Val::Str(_)) => {
                return self.unknown(Origin::Rule(rules::COMPARE, "order of texts".into()))
            }
            (x, y) => {
                return Err(Self::mismatch(
                    f,
                    "int or float",
                    format!("{} and {}", kind_text(x), kind_text(y)),
                ))
            }
        };
        Ok(Val::Bool(match op {
            16 => x < y,
            17 => x <= y,
            18 => x > y,
            _ => x >= y,
        }))
    }
    fn arith(&mut self, f: &Frame, op: u8, a: &Val, b: &Val) -> R<Val> {
        let open = |d: &str| Origin::Rule(rules::ARITHMETIC, d.to_string());
        match (op, a, b) {
            (_, Val::Unknown(u), _) | (_, _, Val::Unknown(u)) => Ok(Val::Unknown(*u)),
            (1 | 3 | 5 | 7 | 9, Val::Int(x), Val::Int(y)) => {
                let (x, y) = (*x, *y);
                let r = match op {
                    1 => x.checked_add(y),
                    3 => x.checked_sub(y),
                    5 => x.checked_mul(y),
                    7 | 9 if y == 0 => {
                        return Err(Stop::err(VmError::DivisionByZero {
                            site: Box::new(Self::site(f)),
                        }))
                    }
                    7 => {
                        // Truncating and flooring quotients agree when the
                        // quotient is exact or not negative.
                        if x % y == 0 || (x >= 0) == (y > 0) {
                            x.checked_div(y)
                        } else {
                            return self
                                .unknown(open("integer quotient of operands of different signs"));
                        }
                    }
                    _ => {
                        if x % y == 0 {
                            Some(0)
                        } else if x >= 0 && y > 0 {
                            Some(x % y)
                        } else {
                            return self.unknown(open("integer remainder of a negative operand"));
                        }
                    }
                };
                match r {
                    Some(n) => Ok(Val::Int(n)),
                    None => self.unknown(open("integer overflow")),
                }
            }
            (2 | 4 | 6 | 8, Val::Float(x), Val::Float(y)) => {
                let r = match op {
                    2 => x + y,
                    4 => x - y,
                    6 => x * y,
                    _ => {
                        if *y == 0.0 {
                            return self.unknown(open("float quotient by zero"));
                        }
                        x / y
                    }
                };
                Ok(Val::Float(r))
            }
            (op, x, y) => Err(Self::mismatch(
                f,
                if op % 2 == 1 {
                    "int, int"
                } else {
                    "float, float"
                },
                format!("{}, {}", kind_text(x), kind_text(y)),
            )),
        }
    }
    /// Opcode 14: a value converted to a declared type.
    #[inline(never)]
    fn convert(&mut self, f: &Frame, v: Val, ty: &str) -> R<Val> {
        let open = |d: String| Origin::Rule(rules::NOT_COPY_CAST, d);
        if ty == "var" || matches!(v, Val::Unknown(_)) {
            return Ok(v);
        }
        Ok(match ty {
            "bool" => self.truth_val(&v)?,
            "int" => match v {
                Val::Int(n) => Val::Int(n),
                Val::Bool(b) => Val::Int(i32::from(b)),
                Val::Float(x) if x.fract() == 0.0 && x.abs() < 2_147_483_648.0 => {
                    Val::Int(x as i32)
                }
                other => self.unknown(open(format!("{} to Int", kind_text(&other))))?,
            },
            "float" => match v {
                Val::Float(x) => Val::Float(x),
                Val::Int(n) => Val::Float(n as f32),
                Val::Bool(b) => Val::Float(if b { 1.0 } else { 0.0 }),
                other => self.unknown(open(format!("{} to Float", kind_text(&other))))?,
            },
            "string" => match v {
                Val::Str(s) => Val::Str(s),
                Val::Int(n) => Val::Str(n.to_string()),
                other => self.unknown(open(format!("{} to String", kind_text(&other))))?,
            },
            t if t.ends_with("[]") || t.contains('#') => {
                let simple = |e: &str| matches!(e, "int" | "float" | "bool" | "string" | "var");
                if self.fits(&v, t)? {
                    v
                } else if let (Val::Array(a), Some(e)) = (&v, t.strip_suffix("[]")) {
                    let from = self.world.arrays[*a].elem.clone();
                    if simple(&from) || simple(e) || e.contains('#') || from.contains('#') {
                        return Err(Self::mismatch(f, t, self.kind_of(&v)));
                    }
                    let d = format!("cast of an array of {from} to {t}");
                    self.unknown(open(d))?
                } else {
                    return Err(Self::mismatch(f, t, self.kind_of(&v)));
                }
            }
            t => match v {
                Val::None => Val::None,
                Val::Object(o) => {
                    if self.object_is_a(o, t)? {
                        Val::Object(o)
                    } else {
                        let d = format!("cast of <{}> to {t}", self.world.objects[o].label);
                        self.unknown(open(d))?
                    }
                }
                other => return Err(Self::mismatch(f, t, kind_text(&other))),
            },
        })
    }

    // ---- arrays and structs -------------------------------------------

    fn array(&mut self, f: &mut Frame, l: &Lit) -> R<usize> {
        match self.read(f, l)? {
            Val::Array(a) => Ok(a),
            Val::Unknown(u) => Err(self.block(u, f)),
            Val::None => Err(Stop::err(VmError::NoneOperand {
                site: Box::new(Self::site(f)),
                what: "array".into(),
            })),
            v => Err(Self::mismatch(f, "array", kind_text(&v))),
        }
    }
    /// The array an opcode works on, or `None` when the operand is none
    /// and `none_ok` (the caller's switch for the rule that settles that
    /// opcode on none) is on; otherwise as [`Self::array`].
    fn array_or_none(&mut self, f: &mut Frame, l: &Lit, none_ok: bool) -> R<Option<usize>> {
        match self.read(f, l)? {
            Val::None if none_ok => Ok(None),
            Val::Array(a) => Ok(Some(a)),
            _ => self.array(f, l).map(Some),
        }
    }
    fn index(&self, f: &Frame, a: usize, i: i32, inclusive: bool) -> R<usize> {
        let len = self.world.arrays[a].items.len();
        let ok = i >= 0 && (i as usize) < len + usize::from(inclusive);
        if ok {
            Ok(i as usize)
        } else {
            Err(Stop::err(VmError::IndexOutOfRange {
                site: Box::new(Self::site(f)),
                index: i64::from(i),
                len,
            }))
        }
    }
    fn check_len(&self, f: &Frame, n: usize) -> R<()> {
        if n > self.limits.array_len {
            return Err(Self::bad(f, format!("array of {n} elements")));
        }
        Ok(())
    }
    /// First index from `start` whose item (or member) equals `v`, -1 when
    /// none; undetermined when an earlier comparison is.
    #[inline(never)]
    fn find(&mut self, f: &Frame, a: usize, member: Option<&str>, v: &Val, start: i32) -> R<Val> {
        let items = self.world.arrays[a].items.clone();
        let from = usize::try_from(start.max(0)).unwrap_or(0);
        for (i, x) in items.iter().enumerate().skip(from) {
            let x = match member {
                None => x.clone(),
                Some(m) => match x {
                    Val::Struct(s) => self.world.structs[*s]
                        .members
                        .iter()
                        .find(|k| k.0 == m)
                        .map(|k| k.2.clone())
                        .ok_or_else(|| Self::bad(f, format!("member {m}")))?,
                    Val::None => {
                        return Err(Stop::err(VmError::NoneOperand {
                            site: Box::new(Self::site(f)),
                            what: "struct in a find".into(),
                        }))
                    }
                    other => return Err(Self::mismatch(f, "struct", kind_text(other))),
                },
            };
            match self.equal(&x, v)? {
                Val::Bool(true) => return Ok(Val::Int(i as i32)),
                Val::Bool(false) => {}
                u => return Ok(u),
            }
        }
        if start < 0 {
            return self.unknown(Origin::Rule(rules::ARRAYS, "negative start index".into()));
        }
        Ok(Val::Int(-1))
    }
    #[inline(never)]
    fn new_struct(&mut self, f: &Frame, ty: &str) -> R<Val> {
        let Some((script, name)) = ty.split_once('#') else {
            return Err(Self::mismatch(f, "a struct type", ty));
        };
        let s = self.script(script)?;
        let decl = s
            .structs
            .iter()
            .find(|d| d.name == name)
            .ok_or_else(|| Self::bad(f, format!("struct {ty}")))?
            .clone();
        let mut members = Vec::new();
        for (n, t, l) in &decl.members {
            let v = self.initial(l, t);
            members.push((n.clone(), t.clone(), v));
        }
        if self.world.structs.len() >= self.limits.values {
            return Err(Self::bad(f, "too many structs"));
        }
        self.world.structs.push(StructVal {
            type_name: ty.to_string(),
            members,
        });
        Ok(Val::Struct(self.world.structs.len() - 1))
    }
    fn struct_slot(&mut self, f: &mut Frame, l: &Lit, member: &Lit) -> R<(usize, usize)> {
        let s = match self.read(f, l)? {
            Val::Struct(s) => s,
            Val::Unknown(u) => return Err(self.block(u, f)),
            Val::None => {
                return Err(Stop::err(VmError::NoneOperand {
                    site: Box::new(Self::site(f)),
                    what: "struct".into(),
                }))
            }
            v => return Err(Self::mismatch(f, "struct", kind_text(&v))),
        };
        let m = Self::ident(f, member)?;
        let k = self.world.structs[s]
            .members
            .iter()
            .position(|x| x.0 == m)
            .ok_or_else(|| Self::bad(f, format!("member {m}")))?;
        Ok((s, k))
    }

    // ---- one instruction ----------------------------------------------

    /// Runs one instruction; `Some` when the function returns.
    #[inline(never)]
    fn step(&mut self, f: &mut Frame, ins: &Ins) -> R<Option<Val>> {
        let a = ins.args.as_slice();
        let arg = |i: usize| slot(a, i);
        let op = self.variant.map(ins.op);
        match op {
            1..=9 => {
                let x = self.read(f, arg(1)?)?;
                let y = self.read(f, arg(2)?)?;
                let v = self.arith(f, op, &x, &y)?;
                self.write(f, arg(0)?, v)?;
            }
            11 | 12 => {
                let v = match (op, self.read(f, arg(1)?)?) {
                    (_, Val::Unknown(u)) => Val::Unknown(u),
                    (11, Val::Int(n)) => match n.checked_neg() {
                        Some(n) => Val::Int(n),
                        None => self
                            .unknown(Origin::Rule(rules::ARITHMETIC, "integer overflow".into()))?,
                    },
                    (12, Val::Float(x)) => Val::Float(-x),
                    (_, v) => return Err(Self::mismatch(f, "number", kind_text(&v))),
                };
                self.write(f, arg(0)?, v)?;
            }
            10 => {
                let v = self.read(f, arg(1)?)?;
                let v = match self.truth_val(&v)? {
                    Val::Bool(b) => Val::Bool(!b),
                    u => u,
                };
                self.write(f, arg(0)?, v)?;
            }
            13 => {
                let v = self.read(f, arg(1)?)?;
                self.write(f, arg(0)?, v)?;
            }
            14 => {
                let v = self.read(f, arg(1)?)?;
                let ty = self.decl_type(f, arg(0)?)?;
                let v = self.convert(f, v, &ty)?;
                self.write(f, arg(0)?, v)?;
            }
            15 => {
                let x = self.read(f, arg(1)?)?;
                let y = self.read(f, arg(2)?)?;
                let mut v = self.equal(&x, &y)?;
                if self.variant == crate::Variant::EqualNegated {
                    if let Val::Bool(b) = v {
                        v = Val::Bool(!b);
                    }
                }
                self.write(f, arg(0)?, v)?;
            }
            16..=19 => {
                let x = self.read(f, arg(1)?)?;
                let y = self.read(f, arg(2)?)?;
                let v = self.order(f, op, &x, &y)?;
                self.write(f, arg(0)?, v)?;
            }
            20 => {
                let Lit::Int(d) = arg(0)? else {
                    return Err(Self::bad(f, "jump target"));
                };
                return self.jump(f, *d).map(|_| None);
            }
            21 | 22 => {
                let Lit::Int(d) = arg(1)? else {
                    return Err(Self::bad(f, "jump target"));
                };
                let d = *d;
                let v = self.read(f, arg(0)?)?;
                let t = match self.truth(&v) {
                    Truth::Known(b) => b,
                    Truth::EmptyArray => {
                        return Err(Stop::blocked(Block {
                            on: Origin::Rule(rules::TRUTH, "truth of an empty array".into()),
                            at: Some(Self::site(f)),
                        }))
                    }
                    Truth::Unknown(u) => return Err(self.block(u, f)),
                };
                if t == (op == 21) {
                    return self.jump(f, d).map(|_| None);
                }
            }
            23 => self.op_call_method(f, ins)?,
            24 => self.op_call_parent(f, ins)?,
            25 => self.op_call_static(f, ins)?,
            26 => {
                let v = self.read(f, arg(0)?)?;
                return Ok(Some(v));
            }
            27 => {
                let x = self.read(f, arg(1)?)?;
                let y = self.read(f, arg(2)?)?;
                let v = match (x, y) {
                    (Val::Unknown(u), _) | (_, Val::Unknown(u)) => Val::Unknown(u),
                    (Val::Str(x), Val::Str(y)) => Val::Str(x + &y),
                    (x, y) => {
                        return Err(Self::mismatch(
                            f,
                            "string, string",
                            format!("{}, {}", kind_text(&x), kind_text(&y)),
                        ))
                    }
                };
                self.write(f, arg(0)?, v)?;
            }
            28 | 29 => {
                let name = Self::ident(f, arg(0)?)?.to_string();
                let inst = self.property_instance(f, arg(1)?, &name)?;
                if op == 28 {
                    let v = self.property_access(f, inst, &name, None)?;
                    self.write(f, arg(2)?, v)?;
                } else {
                    let v = self.read(f, arg(2)?)?;
                    self.property_access(f, inst, &name, Some(v))?;
                }
            }
            30 => {
                let ty = self.decl_type(f, arg(0)?)?;
                let Some(elem) = ty.strip_suffix("[]").map(str::to_string) else {
                    return Err(Self::mismatch(f, "an array type", ty));
                };
                let n = self.int_arg(f, arg(1)?)?;
                let n = usize::try_from(n).map_err(|_| Self::bad(f, format!("array size {n}")))?;
                self.check_len(f, n)?;
                let d = self.default_of(&elem);
                if self.world.arrays.len() >= self.limits.values {
                    return Err(Self::bad(f, "too many arrays"));
                }
                let v = self.world.array(&elem, vec![d; n]);
                self.write(f, arg(0)?, v)?;
            }
            31 => {
                let v = match self.read(f, arg(1)?)? {
                    Val::Array(x) => Val::Int(self.world.arrays[x].items.len() as i32),
                    Val::Unknown(u) => Val::Unknown(u),
                    Val::None if self.none_arrays.length => {
                        self.rules.insert(rules::NONE_ARRAY_LENGTH);
                        Val::Int(0)
                    }
                    Val::None => {
                        self.unknown(Origin::Rule(rules::ARRAYS, "length of none".into()))?
                    }
                    v => return Err(Self::mismatch(f, "array", kind_text(&v))),
                };
                self.write(f, arg(0)?, v)?;
            }
            32 => {
                let Some(x) = self.array_or_none(f, arg(1)?, self.none_arrays.read)? else {
                    // The element type's default (`pex_array_none_read_v1`).
                    self.rules.insert(rules::NONE_ARRAY_READ);
                    let ty = self.decl_type(f, arg(1)?)?;
                    let v = match ty.strip_suffix("[]") {
                        Some(elem) => self.default_of(elem),
                        None => self.unknown(Origin::Rule(
                            rules::NONE_ARRAY_READ,
                            format!("element of none typed {ty}"),
                        ))?,
                    };
                    self.write(f, arg(0)?, v)?;
                    f.pc += 1;
                    return Ok(None);
                };
                let i = self.int_arg(f, arg(2)?)?;
                let i = self.index(f, x, i, false)?;
                let v = self.world.arrays[x].items[i].clone();
                self.write(f, arg(0)?, v)?;
            }
            33 => {
                let Some(x) = self.array_or_none(f, arg(0)?, self.none_arrays.change)? else {
                    self.rules.insert(rules::NONE_ARRAY_CHANGE);
                    f.pc += 1;
                    return Ok(None);
                };
                let i = self.int_arg(f, arg(1)?)?;
                let i = self.index(f, x, i, false)?;
                let v = self.read(f, arg(2)?)?;
                let elem = self.world.arrays[x].elem.clone();
                if !self.fits(&v, &elem)? {
                    return Err(Self::mismatch(f, elem, self.kind_of(&v)));
                }
                self.world.arrays[x].items[i] = v;
            }
            34 => {
                let Some(x) = self.array_or_none(f, arg(0)?, self.none_arrays.find)? else {
                    // Not found (`pex_array_none_find_v1`).
                    self.rules.insert(rules::NONE_ARRAY_FIND);
                    self.write(f, arg(1)?, Val::Int(-1))?;
                    f.pc += 1;
                    return Ok(None);
                };
                let v = self.read(f, arg(2)?)?;
                let start = self.int_arg(f, arg(3)?)?;
                let r = self.find(f, x, None, &v, start)?;
                self.write(f, arg(1)?, r)?;
            }
            40 => {
                let Some(x) = self.array_or_none(f, arg(0)?, self.none_arrays.find)? else {
                    // Not found (`pex_array_none_find_v1`).
                    self.rules.insert(rules::NONE_ARRAY_FIND);
                    self.write(f, arg(1)?, Val::Int(-1))?;
                    f.pc += 1;
                    return Ok(None);
                };
                let Lit::Str(m) = arg(2)? else {
                    return Err(Self::bad(f, "member name"));
                };
                let m = m.to_ascii_lowercase();
                let v = self.read(f, arg(3)?)?;
                let start = self.int_arg(f, arg(4)?)?;
                let r = self.find(f, x, Some(&m), &v, start)?;
                self.write(f, arg(1)?, r)?;
            }
            42 => {
                let Some(x) = self.array_or_none(f, arg(0)?, self.none_arrays.change)? else {
                    self.rules.insert(rules::NONE_ARRAY_CHANGE);
                    f.pc += 1;
                    return Ok(None);
                };
                let v = self.read(f, arg(1)?)?;
                let n = self.int_arg(f, arg(2)?)?;
                let n = usize::try_from(n).map_err(|_| Self::bad(f, format!("count {n}")))?;
                let len = self.world.arrays[x].items.len();
                self.check_len(f, len.saturating_add(n))?;
                let elem = self.world.arrays[x].elem.clone();
                if !self.fits(&v, &elem)? {
                    return Err(Self::mismatch(f, elem, self.kind_of(&v)));
                }
                self.world.arrays[x].items.extend(std::iter::repeat_n(v, n));
            }
            43 => {
                let Some(x) = self.array_or_none(f, arg(0)?, self.none_arrays.change)? else {
                    self.rules.insert(rules::NONE_ARRAY_CHANGE);
                    f.pc += 1;
                    return Ok(None);
                };
                let v = self.read(f, arg(1)?)?;
                let i = self.int_arg(f, arg(2)?)?;
                let i = self.index(f, x, i, true)?;
                let len = self.world.arrays[x].items.len();
                self.check_len(f, len + 1)?;
                let elem = self.world.arrays[x].elem.clone();
                if !self.fits(&v, &elem)? {
                    return Err(Self::mismatch(f, elem, self.kind_of(&v)));
                }
                self.world.arrays[x].items.insert(i, v);
            }
            45 => {
                let Some(x) = self.array_or_none(f, arg(0)?, self.none_arrays.change)? else {
                    self.rules.insert(rules::NONE_ARRAY_CHANGE);
                    f.pc += 1;
                    return Ok(None);
                };
                let i = self.int_arg(f, arg(1)?)?;
                let n = self.int_arg(f, arg(2)?)?;
                let i = self.index(f, x, i, false)?;
                let n = usize::try_from(n).map_err(|_| Self::bad(f, format!("count {n}")))?;
                let len = self.world.arrays[x].items.len();
                if i + n > len {
                    return Err(Stop::err(VmError::IndexOutOfRange {
                        site: Box::new(Self::site(f)),
                        index: (i + n) as i64,
                        len,
                    }));
                }
                self.world.arrays[x].items.drain(i..i + n);
            }
            46 => {
                let Some(x) = self.array_or_none(f, arg(0)?, self.none_arrays.change)? else {
                    self.rules.insert(rules::NONE_ARRAY_CHANGE);
                    f.pc += 1;
                    return Ok(None);
                };
                self.world.arrays[x].items.clear();
            }
            36 => {
                let v = self.read(f, arg(1)?)?;
                let t = Self::ident(f, arg(2)?)?.to_string();
                let r = match v {
                    Val::Unknown(u) => Val::Unknown(u),
                    Val::None => {
                        self.unknown(Origin::Rule(rules::STRUCTS, format!("whether none is {t}")))?
                    }
                    Val::Object(o) => {
                        if self.object_is_a(o, &t)? {
                            Val::Bool(true)
                        } else if self.attach.is_some()
                            && self.world.objects[o].signature_type.is_some()
                        {
                            // A record handle is its record type's engine
                            // type and the scripts attached to it, nothing
                            // else (`vm_record_signature_type_v1`).
                            self.rules.insert(rules::SIGNATURE_TYPE);
                            Val::Bool(false)
                        } else {
                            let d = format!("whether <{}> is {t}", self.world.objects[o].label);
                            self.unknown(Origin::Rule(rules::STRUCTS, d))?
                        }
                    }
                    other => {
                        let k = self.kind_of(&other);
                        Val::Bool(k == t)
                    }
                };
                self.write(f, arg(0)?, r)?;
            }
            37 => {
                let ty = self.decl_type(f, arg(0)?)?;
                let v = self.new_struct(f, &ty)?;
                self.write(f, arg(0)?, v)?;
            }
            38 => {
                let (s, k) = self.struct_slot(f, arg(1)?, arg(2)?)?;
                let v = self.world.structs[s].members[k].2.clone();
                self.write(f, arg(0)?, v)?;
            }
            39 => {
                let (s, k) = self.struct_slot(f, arg(0)?, arg(1)?)?;
                let v = self.read(f, arg(2)?)?;
                let ty = self.world.structs[s].members[k].1.clone();
                if !self.fits(&v, &ty)? {
                    return Err(Self::mismatch(f, ty, self.kind_of(&v)));
                }
                self.world.structs[s].members[k].2 = v;
            }
            _ => {
                return Err(Stop::err(VmError::UnmeasuredOpcode {
                    site: Box::new(Self::site(f)),
                }))
            }
        }
        f.pc += 1;
        Ok(None)
    }
    fn jump(&mut self, f: &mut Frame, d: i32) -> R<()> {
        let t = f.pc as i64 + i64::from(d);
        if t < 0 || t > f.func.code.len() as i64 || d == 0 {
            return Err(Self::bad(f, format!("jump by {d}")));
        }
        f.pc = t as usize;
        Ok(())
    }
}
