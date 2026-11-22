#![forbid(unsafe_code)]
//! Bounded reader for the compiled script files (`.pex`) that Fallout 4
//! stores under `scripts/` in its archives.
//!
//! Every field this reader takes and every rule it enforces was measured over
//! all the `.pex` entries of the archives of one installation; `spec/pex.md`
//! gives each one with its count, and `spec/pex-census.txt` holds the census
//! aggregates. The overall layout was recognised from prior general knowledge
//! before it was measured (see `docs/provenance.md`); it is kept only where a
//! census that could refute it holds. No compiler, decompiler, disassembler,
//! virtual machine, format documentation or script source was consulted.
//!
//! A file is read front to back and must end exactly where the last object
//! ends: every byte belongs to a typed field. Instruction arities come from
//! [`OPCODES`], a table established by the census (for every stored opcode
//! but one, its arity is the only one among the alternatives tried under
//! which every file holding it is read exactly; opcode 34 is the open case,
//! see `spec/pex.md`). An opcode's meaning is claimed only in
//! [`ROLES`] where the data supports it; the others are named `OpNN`.
//! A file outside the measured profile is a named error carrying the file
//! offset; nothing here panics.
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

mod disasm;
pub mod index;
mod roles;
pub use disasm::disassemble;
pub use roles::{op_name, target_slot, Role, ROLES};

/// The first four bytes of every measured file, read as a little-endian u32.
pub const MAGIC: u32 = 0xFA57_C0DE;
/// Bytes 4 and 5 of every measured file.
pub const VERSION: [u8; 2] = [3, 9];
/// The u16 at byte 6 of every measured file.
pub const GAME: u16 = 2;

/// Number of argument slots and whether a counted list follows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Shape {
    /// Arguments always present.
    pub fixed: u8,
    /// When true, the last fixed argument is an integer n and n more
    /// arguments follow.
    pub counted: bool,
}
const fn s(fixed: u8) -> Shape {
    Shape {
        fixed,
        counted: false,
    }
}
const fn c(fixed: u8) -> Shape {
    Shape {
        fixed,
        counted: true,
    }
}
/// Arity of opcodes 0 to 46 (`spec/pex.md`, Opcodes). Each present entry is
/// the only shape, among the 12 alternatives the census tries, under which
/// every file holding the opcode is read exactly, except opcode 34, whose
/// slot 3 is 0 in every instruction so that a counted list after it reads
/// the same bytes. Opcodes 0, 35, 41 and 44
/// occur in no measured file: their arity is unmeasured and the reader refuses
/// them, as it refuses every opcode from 47 up.
pub const OPCODES: [Option<Shape>; 47] = [
    None,
    Some(s(3)),
    Some(s(3)),
    Some(s(3)),
    Some(s(3)),
    Some(s(3)),
    Some(s(3)),
    Some(s(3)),
    Some(s(3)),
    Some(s(3)),
    Some(s(2)),
    Some(s(2)),
    Some(s(2)),
    Some(s(2)),
    Some(s(2)),
    Some(s(3)),
    Some(s(3)),
    Some(s(3)),
    Some(s(3)),
    Some(s(3)),
    Some(s(1)),
    Some(s(2)),
    Some(s(2)),
    Some(c(4)),
    Some(c(3)),
    Some(c(4)),
    Some(s(1)),
    Some(s(3)),
    Some(s(3)),
    Some(s(3)),
    Some(s(2)),
    Some(s(2)),
    Some(s(3)),
    Some(s(3)),
    Some(s(4)),
    None,
    Some(s(3)),
    Some(s(1)),
    Some(s(3)),
    Some(s(3)),
    Some(s(5)),
    None,
    Some(s(3)),
    Some(s(3)),
    None,
    Some(s(3)),
    Some(s(1)),
];

/// Index into the string table, checked against its length.
pub type Str = u16;

/// A typed stored value: a kind byte and its payload.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Value {
    /// Kind 0, no payload.
    None,
    /// Kind 1, a string index read as a name.
    Ident(Str),
    /// Kind 2, a string index read as text.
    Str(Str),
    /// Kind 3, a little-endian i32.
    Int(i32),
    /// Kind 4, a little-endian f32 (bits kept as stored).
    Float(f32),
    /// Kind 5, one byte, 0 or 1 in every measured file.
    Bool(bool),
}
impl Value {
    /// Stored bytes: the kind byte and its payload.
    pub fn stored_len(&self) -> usize {
        match self {
            Value::None => 1,
            Value::Ident(_) | Value::Str(_) => 3,
            Value::Int(_) | Value::Float(_) => 5,
            Value::Bool(_) => 2,
        }
    }
    /// The stored kind byte.
    pub fn kind(&self) -> u8 {
        match self {
            Value::None => 0,
            Value::Ident(_) => 1,
            Value::Str(_) => 2,
            Value::Int(_) => 3,
            Value::Float(_) => 4,
            Value::Bool(_) => 5,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Header<'a> {
    pub major: u8,
    pub minor: u8,
    pub game: u16,
    /// u64 at byte 8.
    pub compiled: u64,
    /// The three length-prefixed texts after it.
    pub source: &'a [u8],
    pub user: &'a [u8],
    pub machine: &'a [u8],
}

#[derive(Debug, Clone, Serialize)]
pub struct DebugFunction {
    pub object: Str,
    pub state: Str,
    pub name: Str,
    /// 0, 1 or 2 in every measured file.
    pub kind: u8,
    /// One u16 per instruction of the function it names.
    pub lines: Vec<u16>,
}
#[derive(Debug, Clone, Serialize)]
pub struct PropertyGroup {
    pub object: Str,
    pub name: Str,
    pub doc: Str,
    pub user_flags: u32,
    pub properties: Vec<Str>,
}
#[derive(Debug, Clone, Serialize)]
pub struct StructOrder {
    pub object: Str,
    pub name: Str,
    pub members: Vec<Str>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Debug {
    /// u64 after the presence byte.
    pub modified: u64,
    pub functions: Vec<DebugFunction>,
    pub property_groups: Vec<PropertyGroup>,
    pub struct_orders: Vec<StructOrder>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserFlag {
    pub name: Str,
    pub bit: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct Member {
    pub name: Str,
    pub type_name: Str,
    pub user_flags: u32,
    pub value: Value,
    /// A byte, 0 in every measured member; the name is a label.
    pub constant: u8,
    pub doc: Str,
}
#[derive(Debug, Clone, Serialize)]
pub struct Struct {
    pub name: Str,
    pub members: Vec<Member>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Variable {
    pub name: Str,
    pub type_name: Str,
    pub user_flags: u32,
    pub value: Value,
    /// A byte, 0 or 1; mostly 1 for variables a property names, but not
    /// only (`spec/pex.md`); the name is a label, nothing reads it.
    pub constant: u8,
}
/// Bit of [`Property::flags`] whose functions follow: read.
pub const PROPERTY_READ: u8 = 1;
/// Bit of [`Property::flags`] whose functions follow: write.
pub const PROPERTY_WRITE: u8 = 2;
/// Bit of [`Property::flags`] after which a variable name follows instead of
/// functions.
pub const PROPERTY_VARIABLE: u8 = 4;
#[derive(Debug, Clone, Serialize)]
pub struct Property {
    pub name: Str,
    pub type_name: Str,
    pub doc: Str,
    pub user_flags: u32,
    pub flags: u8,
    /// Present when bit 4 of `flags` is set.
    pub variable: Option<Str>,
    /// Present when bit 4 is clear and bit 1 is set.
    pub read: Option<Function>,
    /// Present when bit 4 is clear and bit 2 is set.
    pub write: Option<Function>,
}
#[derive(Debug, Clone, Serialize)]
pub struct NamedFunction {
    pub name: Str,
    pub function: Function,
}
#[derive(Debug, Clone, Serialize)]
pub struct State {
    pub name: Str,
    pub functions: Vec<NamedFunction>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Object {
    pub name: Str,
    /// File offset of the size word.
    pub offset: u32,
    /// The stored size: bytes from the size word to the object's end.
    pub size: u32,
    pub parent: Str,
    pub doc: Str,
    /// A byte, 0 or 1; the name is a label, nothing reads it.
    pub constant: u8,
    pub user_flags: u32,
    pub auto_state: Str,
    pub structs: Vec<Struct>,
    pub variables: Vec<Variable>,
    pub properties: Vec<Property>,
    pub states: Vec<State>,
}
/// Bit of [`Function::flags`]: no receiver (the functions so marked are
/// the targets of opcode 25's calls in the census).
pub const FUNCTION_GLOBAL: u8 = 1;
/// Bit of [`Function::flags`]: no instructions in every measured function
/// that has it.
pub const FUNCTION_NATIVE: u8 = 2;
#[derive(Debug, Clone, Serialize)]
pub struct Function {
    pub return_type: Str,
    pub doc: Str,
    pub user_flags: u32,
    pub flags: u8,
    /// (name, type) pairs.
    pub params: Vec<(Str, Str)>,
    /// (name, type) pairs.
    pub locals: Vec<(Str, Str)>,
    pub code: Vec<Instruction>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Instruction {
    /// File offset of the opcode byte.
    pub offset: u32,
    pub op: u8,
    /// The fixed arguments, the count included for counted opcodes.
    pub args: Vec<Value>,
    /// The counted arguments.
    pub extra: Vec<Value>,
}

/// File offsets where each part starts; the header runs from 0 to
/// `strings`, and `end` is the file length.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Sections {
    pub strings: u32,
    /// The debug presence byte.
    pub debug: u32,
    pub user_flags: u32,
    pub objects: u32,
    pub end: u32,
}

/// A whole `.pex` file.
#[derive(Debug, Clone, Serialize)]
pub struct Pex<'a> {
    pub header: Header<'a>,
    pub sections: Sections,
    pub strings: Vec<&'a [u8]>,
    pub debug: Option<Debug>,
    pub user_flags: Vec<UserFlag>,
    pub objects: Vec<Object>,
}

struct R<'a, 'b> {
    d: &'a [u8],
    p: usize,
    file: &'b str,
    b: &'b Budget,
    strings: usize,
    ops: &'b [Option<Shape>],
}
impl<'a> R<'a, '_> {
    fn err(&self, code: &str, at: usize, expected: impl ToString, found: impl ToString) -> Error {
        Error::new(code, self.file, at as u64, expected, found)
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .p
            .checked_add(n)
            .filter(|e| *e <= self.d.len())
            .ok_or_else(|| {
                self.err(
                    "Truncated",
                    self.p,
                    format!("{n} bytes"),
                    self.d.len() - self.p,
                )
            })?;
        let v = &self.d[self.p..end];
        self.p = end;
        Ok(v)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        let v = self.take(2)?;
        Ok(u16::from_le_bytes([v[0], v[1]]))
    }
    fn u32(&mut self) -> Result<u32> {
        let v = self.take(4)?;
        Ok(u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
    }
    fn u64(&mut self) -> Result<u64> {
        let v = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(v);
        Ok(u64::from_le_bytes(a))
    }
    fn text(&mut self) -> Result<&'a [u8]> {
        let n = self.u16()?;
        self.take(n as usize)
    }
    fn s(&mut self) -> Result<Str> {
        let at = self.p;
        let i = self.u16()?;
        if usize::from(i) >= self.strings {
            return Err(self.err("StringIndex", at, format!("< {}", self.strings), i));
        }
        Ok(i)
    }
    /// A count followed by elements of at least `min` bytes each, checked
    /// against the bytes left before anything is reserved.
    fn vec<T>(&mut self, min: usize) -> Result<(usize, Vec<T>)> {
        let at = self.p;
        let n = usize::from(self.u16()?);
        let need = n.saturating_mul(min);
        if need > self.d.len() - self.p {
            return Err(self.err(
                "Truncated",
                at,
                format!("{n} elements of at least {min} bytes"),
                self.d.len() - self.p,
            ));
        }
        let mut v = Vec::new();
        self.b.reserve(&mut v, n, self.file, at as u64)?;
        Ok((n, v))
    }
    fn value(&mut self) -> Result<Value> {
        let at = self.p;
        Ok(match self.u8()? {
            0 => Value::None,
            1 => Value::Ident(self.s()?),
            2 => Value::Str(self.s()?),
            3 => Value::Int(self.u32()? as i32),
            4 => Value::Float(f32::from_bits(self.u32()?)),
            5 => {
                let v = self.u8()?;
                if v > 1 {
                    return Err(self.err("BoolValue", at + 1, "0 or 1", v));
                }
                Value::Bool(v == 1)
            }
            k => return Err(self.err("ValueKind", at, "0..=5", k)),
        })
    }
    fn pairs(&mut self) -> Result<Vec<(Str, Str)>> {
        let (n, mut v) = self.vec(4)?;
        for _ in 0..n {
            v.push((self.s()?, self.s()?));
        }
        Ok(v)
    }
    fn function(&mut self) -> Result<Function> {
        let return_type = self.s()?;
        let doc = self.s()?;
        let user_flags = self.u32()?;
        let at = self.p;
        let flags = self.u8()?;
        if flags & !(FUNCTION_GLOBAL | FUNCTION_NATIVE) != 0 {
            return Err(self.err("FunctionFlags", at, "bits 1 and 2 only", flags));
        }
        let params = self.pairs()?;
        let locals = self.pairs()?;
        let (n, mut code) = self.vec(1)?;
        for _ in 0..n {
            code.push(self.instruction()?);
        }
        Ok(Function {
            return_type,
            doc,
            user_flags,
            flags,
            params,
            locals,
            code,
        })
    }
    fn instruction(&mut self) -> Result<Instruction> {
        let offset = self.p;
        let op = self.u8()?;
        let shape = self
            .ops
            .get(usize::from(op))
            .copied()
            .flatten()
            .ok_or_else(|| {
                self.err(
                    "Opcode",
                    offset,
                    "an opcode stored in the measured files",
                    op,
                )
            })?;
        let mut args = Vec::new();
        self.b.reserve(
            &mut args,
            usize::from(shape.fixed),
            self.file,
            offset as u64,
        )?;
        for _ in 0..shape.fixed {
            args.push(self.value()?);
        }
        let mut extra = Vec::new();
        if shape.counted {
            let n = match args.last() {
                Some(Value::Int(n)) if *n >= 0 => *n as usize,
                other => {
                    return Err(self.err(
                        "ArgumentCount",
                        offset,
                        "a non-negative integer as the last fixed argument",
                        format!("{other:?}"),
                    ))
                }
            };
            // Each counted argument takes at least one byte.
            if n > self.d.len() - self.p {
                return Err(self.err(
                    "Truncated",
                    self.p,
                    format!("{n} counted arguments"),
                    self.d.len() - self.p,
                ));
            }
            self.b.reserve(&mut extra, n, self.file, offset as u64)?;
            for _ in 0..n {
                extra.push(self.value()?);
            }
        }
        Ok(Instruction {
            offset: offset as u32,
            op,
            args,
            extra,
        })
    }
    fn object(&mut self) -> Result<Object> {
        let name = self.s()?;
        let offset = self.p;
        let size = self.u32()?;
        let parent = self.s()?;
        let doc = self.s()?;
        let constant = self.u8()?;
        let user_flags = self.u32()?;
        let auto_state = self.s()?;
        let (n, mut structs) = self.vec(4)?;
        for _ in 0..n {
            let name = self.s()?;
            let (m, mut members) = self.vec(12)?;
            for _ in 0..m {
                members.push(Member {
                    name: self.s()?,
                    type_name: self.s()?,
                    user_flags: self.u32()?,
                    value: self.value()?,
                    constant: self.u8()?,
                    doc: self.s()?,
                });
            }
            structs.push(Struct { name, members });
        }
        let (n, mut variables) = self.vec(10)?;
        for _ in 0..n {
            variables.push(Variable {
                name: self.s()?,
                type_name: self.s()?,
                user_flags: self.u32()?,
                value: self.value()?,
                constant: self.u8()?,
            });
        }
        let (n, mut properties) = self.vec(11)?;
        for _ in 0..n {
            let name = self.s()?;
            let type_name = self.s()?;
            let doc = self.s()?;
            let user_flags = self.u32()?;
            let at = self.p;
            let flags = self.u8()?;
            if flags & !(PROPERTY_READ | PROPERTY_WRITE | PROPERTY_VARIABLE) != 0 {
                return Err(self.err("PropertyFlags", at, "bits 1, 2 and 4 only", flags));
            }
            let (variable, read, write) = if flags & PROPERTY_VARIABLE != 0 {
                (Some(self.s()?), None, None)
            } else {
                let read = if flags & PROPERTY_READ != 0 {
                    Some(self.function()?)
                } else {
                    None
                };
                let write = if flags & PROPERTY_WRITE != 0 {
                    Some(self.function()?)
                } else {
                    None
                };
                (None, read, write)
            };
            properties.push(Property {
                name,
                type_name,
                doc,
                user_flags,
                flags,
                variable,
                read,
                write,
            });
        }
        let (n, mut states) = self.vec(4)?;
        for _ in 0..n {
            let name = self.s()?;
            let (m, mut functions) = self.vec(15)?;
            for _ in 0..m {
                functions.push(NamedFunction {
                    name: self.s()?,
                    function: self.function()?,
                });
            }
            states.push(State { name, functions });
        }
        let stored_end = offset.checked_add(size as usize);
        if stored_end != Some(self.p) {
            return Err(self.err(
                "ObjectSize",
                offset,
                format!("object end {}", self.p),
                format!("size {size}"),
            ));
        }
        Ok(Object {
            name,
            offset: offset as u32,
            size,
            parent,
            doc,
            constant,
            user_flags,
            auto_state,
            structs,
            variables,
            properties,
            states,
        })
    }
    fn debug(&mut self) -> Result<Debug> {
        let modified = self.u64()?;
        let (n, mut functions) = self.vec(9)?;
        for _ in 0..n {
            let object = self.s()?;
            let state = self.s()?;
            let name = self.s()?;
            let at = self.p;
            let kind = self.u8()?;
            if kind > 2 {
                return Err(self.err("DebugFunctionKind", at, "0..=2", kind));
            }
            let (m, mut lines) = self.vec(2)?;
            for _ in 0..m {
                lines.push(self.u16()?);
            }
            functions.push(DebugFunction {
                object,
                state,
                name,
                kind,
                lines,
            });
        }
        let (n, mut property_groups) = self.vec(12)?;
        for _ in 0..n {
            let object = self.s()?;
            let name = self.s()?;
            let doc = self.s()?;
            let user_flags = self.u32()?;
            let (m, mut properties) = self.vec(2)?;
            for _ in 0..m {
                properties.push(self.s()?);
            }
            property_groups.push(PropertyGroup {
                object,
                name,
                doc,
                user_flags,
                properties,
            });
        }
        let (n, mut struct_orders) = self.vec(6)?;
        for _ in 0..n {
            let object = self.s()?;
            let name = self.s()?;
            let (m, mut members) = self.vec(2)?;
            for _ in 0..m {
                members.push(self.s()?);
            }
            struct_orders.push(StructOrder {
                object,
                name,
                members,
            });
        }
        Ok(Debug {
            modified,
            functions,
            property_groups,
            struct_orders,
        })
    }
}

impl<'a> Pex<'a> {
    /// Reads a whole file with the measured opcode table.
    pub fn read(d: &'a [u8], file: &str, b: &Budget) -> Result<Self> {
        Self::read_with(d, file, b, &OPCODES)
    }
    /// Reads a whole file with a given opcode table (the census uses this to
    /// try alternative arities).
    pub fn read_with(d: &'a [u8], file: &str, b: &Budget, ops: &[Option<Shape>]) -> Result<Self> {
        let mut r = R {
            d,
            p: 0,
            file,
            b,
            strings: 0,
            ops,
        };
        let magic = r.u32()?;
        if magic != MAGIC {
            return Err(r.err("Magic", 0, format!("{MAGIC:#x}"), format!("{magic:#x}")));
        }
        let major = r.u8()?;
        let minor = r.u8()?;
        if [major, minor] != VERSION {
            return Err(r.err("Version", 4, "3.9", format!("{major}.{minor}")));
        }
        let game = r.u16()?;
        if game != GAME {
            return Err(r.err("Game", 6, GAME, game));
        }
        let compiled = r.u64()?;
        let header = Header {
            major,
            minor,
            game,
            compiled,
            source: r.text()?,
            user: r.text()?,
            machine: r.text()?,
        };
        let strings_at = r.p;
        let (n, mut strings) = r.vec(2)?;
        for _ in 0..n {
            strings.push(r.text()?);
        }
        r.strings = strings.len();
        let at = r.p;
        let debug_at = r.p;
        let debug = match r.u8()? {
            0 => None,
            1 => Some(r.debug()?),
            v => return Err(r.err("DebugPresence", at, "0 or 1", v)),
        };
        let user_flags_at = r.p;
        let (n, mut user_flags) = r.vec(3)?;
        for _ in 0..n {
            user_flags.push(UserFlag {
                name: r.s()?,
                bit: r.u8()?,
            });
        }
        let objects_at = r.p;
        let (n, mut objects) = r.vec(20)?;
        for _ in 0..n {
            objects.push(r.object()?);
        }
        if r.p != d.len() {
            return Err(r.err("TrailingBytes", r.p, "end of file", d.len() - r.p));
        }
        let sections = Sections {
            strings: strings_at as u32,
            debug: debug_at as u32,
            user_flags: user_flags_at as u32,
            objects: objects_at as u32,
            end: d.len() as u32,
        };
        Ok(Pex {
            header,
            sections,
            strings,
            debug,
            user_flags,
            objects,
        })
    }
    /// The string at an index (indices are checked when read).
    pub fn str(&self, i: Str) -> &'a [u8] {
        self.strings.get(usize::from(i)).copied().unwrap_or(b"")
    }
    /// Every function with where it sits: (object, state or property, name, function).
    pub fn functions(&self) -> Vec<FunctionSite<'_>> {
        let mut out = Vec::new();
        for (oi, o) in self.objects.iter().enumerate() {
            for p in &o.properties {
                if let Some(f) = &p.read {
                    out.push(FunctionSite {
                        object: oi,
                        place: Place::Read(p.name),
                        function: f,
                    });
                }
                if let Some(f) = &p.write {
                    out.push(FunctionSite {
                        object: oi,
                        place: Place::Write(p.name),
                        function: f,
                    });
                }
            }
            for s in &o.states {
                for f in &s.functions {
                    out.push(FunctionSite {
                        object: oi,
                        place: Place::State(s.name, f.name),
                        function: &f.function,
                    });
                }
            }
        }
        out
    }
}
/// Where a function is stored.
impl Instruction {
    /// Stored bytes: the opcode byte and every argument.
    pub fn stored_len(&self) -> usize {
        1 + self
            .args
            .iter()
            .chain(&self.extra)
            .map(Value::stored_len)
            .sum::<usize>()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// In a state: (state name, function name).
    State(Str, Str),
    /// A property's read function: the property name.
    Read(Str),
    /// A property's write function: the property name.
    Write(Str),
}
#[derive(Debug, Clone, Copy)]
pub struct FunctionSite<'p> {
    pub object: usize,
    pub place: Place,
    pub function: &'p Function,
}

#[cfg(test)]
mod tests;
