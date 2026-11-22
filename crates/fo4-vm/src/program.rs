//! Owned, name-resolved copies of the `fo4_pex` structures the interpreter
//! runs: one [`Script`] per `.pex` object, names lower-cased (every name
//! resolution measured in `spec/pex.md` ignores ASCII case), string literals
//! kept as stored.
use crate::VmError;
use fo4_pex::{index::lower, Function, Pex, Value, FUNCTION_GLOBAL, FUNCTION_NATIVE};
use std::{collections::BTreeMap, rc::Rc};

/// An instruction argument as stored.
#[derive(Debug, Clone, PartialEq)]
pub enum Lit {
    None,
    /// A name, lower-cased.
    Ident(String),
    /// A text literal, as stored.
    Str(String),
    Int(i32),
    Float(f32),
    Bool(bool),
}

#[derive(Debug, Clone)]
pub struct Ins {
    pub op: u8,
    /// Fixed arguments (the count included for the counted opcodes).
    pub args: Vec<Lit>,
    /// Counted arguments.
    pub extra: Vec<Lit>,
}

#[derive(Debug, Clone)]
pub struct Func {
    /// Lower-case name of the script that stores the function.
    pub script: String,
    /// Lower-case state name (empty for the empty state); a property's
    /// functions are stored with the empty state.
    pub state: String,
    /// Lower-case function name (`name.get` / `name.set` for a property's
    /// read and write functions).
    pub name: String,
    pub return_type: String,
    pub flags: u8,
    /// (name, type), both lower-case.
    pub params: Vec<(String, String)>,
    pub locals: Vec<(String, String)>,
    pub code: Vec<Ins>,
}
impl Func {
    pub fn native(&self) -> bool {
        self.flags & FUNCTION_NATIVE != 0
    }
    pub fn global(&self) -> bool {
        self.flags & FUNCTION_GLOBAL != 0
    }
}

#[derive(Debug, Clone)]
pub struct VarDecl {
    pub name: String,
    pub type_name: String,
    pub init: Lit,
}
#[derive(Debug, Clone)]
pub struct PropDecl {
    pub name: String,
    pub type_name: String,
    /// The variable behind it (property flag bit 4).
    pub variable: Option<String>,
    pub read: Option<Rc<Func>>,
    pub write: Option<Rc<Func>>,
}
#[derive(Debug, Clone)]
pub struct StructDecl {
    pub name: String,
    /// (name, type, stored initial value).
    pub members: Vec<(String, String, Lit)>,
}

#[derive(Debug, Clone)]
pub struct Script {
    /// Lower-case object name.
    pub name: String,
    /// Lower-case parent name; empty when there is none.
    pub parent: String,
    pub auto_state: String,
    pub variables: Vec<VarDecl>,
    pub properties: Vec<PropDecl>,
    pub structs: Vec<StructDecl>,
    /// Functions by (state, name).
    pub functions: BTreeMap<(String, String), Rc<Func>>,
}
impl Script {
    /// Copies object `object` of a file.
    pub fn from_pex(p: &Pex<'_>, object: usize) -> Option<Script> {
        let o = p.objects.get(object)?;
        let t = |i| lower(p.str(i));
        let lit = |v: &Value| match *v {
            Value::None => Lit::None,
            Value::Ident(i) => Lit::Ident(lower(p.str(i))),
            Value::Str(i) => Lit::Str(String::from_utf8_lossy(p.str(i)).into_owned()),
            Value::Int(n) => Lit::Int(n),
            Value::Float(f) => Lit::Float(f),
            Value::Bool(b) => Lit::Bool(b),
        };
        let name = t(o.name);
        let func = |state: String, fname: String, f: &Function| {
            Rc::new(Func {
                script: name.clone(),
                state,
                name: fname,
                return_type: t(f.return_type),
                flags: f.flags,
                params: f.params.iter().map(|(a, b)| (t(*a), t(*b))).collect(),
                locals: f.locals.iter().map(|(a, b)| (t(*a), t(*b))).collect(),
                code: f
                    .code
                    .iter()
                    .map(|i| Ins {
                        op: i.op,
                        args: i.args.iter().map(lit).collect(),
                        extra: i.extra.iter().map(lit).collect(),
                    })
                    .collect(),
            })
        };
        let mut functions = BTreeMap::new();
        for st in &o.states {
            for f in &st.functions {
                let k = (t(st.name), t(f.name));
                functions.insert(k.clone(), func(k.0, k.1, &f.function));
            }
        }
        Some(Script {
            name: name.clone(),
            parent: t(o.parent),
            auto_state: t(o.auto_state),
            variables: o
                .variables
                .iter()
                .map(|v| VarDecl {
                    name: t(v.name),
                    type_name: t(v.type_name),
                    init: lit(&v.value),
                })
                .collect(),
            properties: o
                .properties
                .iter()
                .map(|pr| {
                    let n = t(pr.name);
                    PropDecl {
                        read: pr
                            .read
                            .as_ref()
                            .map(|f| func(String::new(), format!("{n}.get"), f)),
                        write: pr
                            .write
                            .as_ref()
                            .map(|f| func(String::new(), format!("{n}.set"), f)),
                        variable: pr.variable.map(t),
                        type_name: t(pr.type_name),
                        name: n,
                    }
                })
                .collect(),
            structs: o
                .structs
                .iter()
                .map(|s| StructDecl {
                    name: t(s.name),
                    members: s
                        .members
                        .iter()
                        .map(|m| (t(m.name), t(m.type_name), lit(&m.value)))
                        .collect(),
                })
                .collect(),
            functions,
        })
    }
    /// The function of a name stored in a state.
    pub fn function(&self, state: &str, name: &str) -> Option<&Rc<Func>> {
        self.functions.get(&(state.to_string(), name.to_string()))
    }
    pub fn property(&self, name: &str) -> Option<&PropDecl> {
        self.properties.iter().find(|p| p.name == name)
    }
    pub fn variable(&self, name: &str) -> Option<&VarDecl> {
        self.variables.iter().find(|v| v.name == name)
    }
}

/// Where the interpreter finds scripts by lower-case object name.
pub trait Library {
    /// The script, `None` when no file holds it.
    fn load(&mut self, name: &str) -> Result<Option<Rc<Script>>, VmError>;
}

/// Scripts held in memory (tests, and callers that load files themselves).
#[derive(Default)]
pub struct MapLibrary {
    pub scripts: BTreeMap<String, Rc<Script>>,
}
impl MapLibrary {
    pub fn add(&mut self, s: Script) {
        self.scripts.insert(s.name.clone(), Rc::new(s));
    }
}
impl Library for MapLibrary {
    fn load(&mut self, name: &str) -> Result<Option<Rc<Script>>, VmError> {
        Ok(self.scripts.get(name).cloned())
    }
}
