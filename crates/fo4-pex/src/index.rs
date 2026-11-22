//! Declarations of many scripts by name, and the name resolution the census
//! checks (`spec/pex.md`, Roles): a call's name slot against the functions
//! declared by a type and its parents, a property's name slot against the
//! properties, an identifier against the function's locals and parameters,
//! the object's variables and its parents' variables. Names compare without
//! ASCII case, as every resolution measured does.
use crate::{Function, Instruction, Pex, Role, Value, FUNCTION_GLOBAL, FUNCTION_NATIVE, ROLES};
use std::collections::HashMap;

/// Lower-case text of stored bytes.
pub fn lower(b: &[u8]) -> String {
    String::from_utf8_lossy(b).to_ascii_lowercase()
}

/// Longest parent chain followed; deeper chains end the walk.
pub const MAX_PARENTS: usize = 64;

#[derive(Debug, Clone)]
pub struct FunctionDecl {
    /// Lower-case state name; empty for the empty state.
    pub state: String,
    pub params: usize,
    pub flags: u8,
    pub return_type: String,
}
impl FunctionDecl {
    pub fn native(&self) -> bool {
        self.flags & FUNCTION_NATIVE != 0
    }
    pub fn global(&self) -> bool {
        self.flags & FUNCTION_GLOBAL != 0
    }
}
#[derive(Debug, Clone, Default)]
pub struct ScriptDecl {
    pub parent: String,
    pub functions: HashMap<String, Vec<FunctionDecl>>,
    pub properties: HashMap<String, String>,
    pub variables: HashMap<String, String>,
    pub structs: HashMap<String, Vec<String>>,
}

/// Declarations by lower-case object name.
#[derive(Debug, Default)]
pub struct Index {
    pub scripts: HashMap<String, ScriptDecl>,
}

/// What an identifier names inside a function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Local,
    Param,
    Variable,
    ParentVariable,
    SelfRef,
    /// None of the above (a function, property, member or type name).
    Other,
}

/// A call's resolved target.
#[derive(Debug, Clone)]
pub struct Callee<'i> {
    /// The script whose declaration was found (the type itself or a parent).
    pub script: String,
    pub name: String,
    pub decl: &'i FunctionDecl,
}

impl Index {
    /// Adds every object of a file.
    pub fn add(&mut self, p: &Pex<'_>) {
        for o in &p.objects {
            let mut s = ScriptDecl {
                parent: lower(p.str(o.parent)),
                ..Default::default()
            };
            for st in &o.states {
                for f in &st.functions {
                    s.functions
                        .entry(lower(p.str(f.name)))
                        .or_default()
                        .push(FunctionDecl {
                            state: lower(p.str(st.name)),
                            params: f.function.params.len(),
                            flags: f.function.flags,
                            return_type: lower(p.str(f.function.return_type)),
                        });
                }
            }
            for pr in &o.properties {
                s.properties
                    .insert(lower(p.str(pr.name)), lower(p.str(pr.type_name)));
            }
            for v in &o.variables {
                s.variables
                    .insert(lower(p.str(v.name)), lower(p.str(v.type_name)));
            }
            for st in &o.structs {
                let m = st.members.iter().map(|m| lower(p.str(m.name))).collect();
                s.structs.insert(lower(p.str(st.name)), m);
            }
            self.scripts.insert(lower(p.str(o.name)), s);
        }
    }
    /// The script and its parents, nearest first, while they are indexed.
    pub fn chain(&self, script: &str) -> Vec<(&str, &ScriptDecl)> {
        let mut out = Vec::new();
        let mut t = script;
        while let Some((k, s)) = self.scripts.get_key_value(t) {
            out.push((k.as_str(), s));
            if s.parent.is_empty() || out.len() >= MAX_PARENTS {
                break;
            }
            t = &s.parent;
        }
        out
    }
    /// A function declared by a script or a parent: the empty-state
    /// declaration of the nearest script that declares the name, else its
    /// first declaration there.
    pub fn function(&self, script: &str, name: &str) -> Option<Callee<'_>> {
        for (k, s) in self.chain(script) {
            if let Some(v) = s.functions.get(name) {
                if let Some(d) = v.iter().find(|f| f.state.is_empty()).or(v.first()) {
                    return Some(Callee {
                        script: k.to_string(),
                        name: name.to_string(),
                        decl: d,
                    });
                }
            }
        }
        None
    }
    /// The type of a property declared by a script or a parent.
    pub fn property(&self, script: &str, name: &str) -> Option<&str> {
        self.chain(script)
            .into_iter()
            .find_map(|(_, s)| s.properties.get(name).map(String::as_str))
    }
    /// The member names of a struct type written `script#struct`.
    pub fn struct_members(&self, ty: &str) -> Option<&[String]> {
        let (script, st) = ty.split_once('#')?;
        self.scripts.get(script)?.structs.get(st).map(Vec::as_slice)
    }
}

/// Names visible inside one function, with their declared types.
pub struct Frame<'p> {
    p: &'p Pex<'p>,
    object: String,
    names: HashMap<String, (Scope, String)>,
}
impl<'p> Frame<'p> {
    pub fn new(p: &'p Pex<'p>, object: usize, f: &Function, ix: &Index) -> Self {
        let o = &p.objects[object];
        let mut names = HashMap::new();
        let parents = ix.chain(&lower(p.str(o.parent)));
        for (_, s) in parents.iter().rev() {
            for (n, t) in &s.variables {
                names.insert(n.clone(), (Scope::ParentVariable, t.clone()));
            }
        }
        for v in &o.variables {
            names.insert(
                lower(p.str(v.name)),
                (Scope::Variable, lower(p.str(v.type_name))),
            );
        }
        for (n, t) in &f.params {
            names.insert(lower(p.str(*n)), (Scope::Param, lower(p.str(*t))));
        }
        for (n, t) in &f.locals {
            names.insert(lower(p.str(*n)), (Scope::Local, lower(p.str(*t))));
        }
        Frame {
            p,
            object: lower(p.str(o.name)),
            names,
        }
    }
    /// Stored bytes of a string index.
    pub fn text(&self, i: crate::Str) -> &'p [u8] {
        self.p.str(i)
    }
    /// The object's lower-case name.
    pub fn object(&self) -> &str {
        &self.object
    }
    /// Scope and lower-case declared type of an identifier value; `None` for
    /// other kinds. `self` is the object itself unless a name shadows it.
    pub fn ident(&self, v: &Value) -> Option<(Scope, String)> {
        let n = self.name(v)?;
        if let Some(x) = self.names.get(&n) {
            return Some(x.clone());
        }
        if n == "self" {
            return Some((Scope::SelfRef, self.object.clone()));
        }
        Some((Scope::Other, String::new()))
    }
    /// Lower-case name of an identifier value.
    pub fn name(&self, v: &Value) -> Option<String> {
        match v {
            Value::Ident(i) => Some(lower(self.p.str(*i))),
            _ => None,
        }
    }
    /// The target of a call instruction (roles CallMethod, CallParent and
    /// CallStatic), when its names resolve.
    pub fn callee<'i>(&self, ix: &'i Index, ins: &Instruction) -> Option<Callee<'i>> {
        let a = &ins.args;
        let (ty, name) = match ROLES.get(usize::from(ins.op))? {
            Role::CallMethod => (self.ident(a.get(1)?)?.1, self.name(a.first()?)?),
            Role::CallParent => (
                ix.scripts.get(&self.object)?.parent.clone(),
                self.name(a.first()?)?,
            ),
            Role::CallStatic => (self.name(a.first()?)?, self.name(a.get(1)?)?),
            _ => return None,
        };
        ix.function(&ty, &name)
    }
}
