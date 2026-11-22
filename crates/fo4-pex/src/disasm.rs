//! Text disassembly of one file: every stored field, in stored order, with
//! instructions named by their claimed role or `OpNN` (`spec/pex.md`).
use crate::{
    op_name, target_slot, Function, Pex, Place, Str, Value, FUNCTION_GLOBAL, FUNCTION_NATIVE,
    PROPERTY_VARIABLE,
};
use std::fmt::Write as _;

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

impl Pex<'_> {
    fn t(&self, i: Str) -> String {
        text(self.str(i))
    }
    fn value(&self, v: &Value) -> String {
        match v {
            Value::None => "none".into(),
            Value::Ident(i) => self.t(*i),
            Value::Str(i) => format!("{:?}", self.t(*i)),
            Value::Int(n) => n.to_string(),
            Value::Float(f) => format!("{f:?}"),
            Value::Bool(b) => b.to_string(),
        }
    }
    fn flag_names(&self, v: u32) -> String {
        let mut out = Vec::new();
        for bit in 0..32u32 {
            if v & (1 << bit) != 0 {
                out.push(
                    self.user_flags
                        .iter()
                        .find(|u| u32::from(u.bit) == bit)
                        .map_or_else(|| format!("bit{bit}"), |u| self.t(u.name)),
                );
            }
        }
        if out.is_empty() {
            String::new()
        } else {
            format!(" [{}]", out.join(" "))
        }
    }
    /// The debug line numbers of a function: the entry naming its place
    /// with as many lines as it has instructions, else an entry of the same
    /// name in another state with that count (`spec/pex.md`, Debug).
    pub fn debug_lines(&self, object: usize, place: Place, f: &Function) -> Option<&[u16]> {
        let d = self.debug.as_ref()?;
        let o = self.objects.get(object)?;
        let same = |a: Str, b: Str| self.str(a).eq_ignore_ascii_case(self.str(b));
        let mine = d
            .functions
            .iter()
            .filter(|e| same(e.object, o.name) && e.lines.len() == f.code.len());
        let mut fallback = None;
        for e in mine {
            match (e.kind, place) {
                (0, Place::State(st, n)) if e.name == n => {
                    if e.state == st {
                        return Some(&e.lines);
                    }
                    fallback.get_or_insert(&e.lines[..]);
                }
                (1, Place::Read(n)) | (2, Place::Write(n)) if e.name == n => return Some(&e.lines),
                _ => {}
            }
        }
        fallback
    }
    fn function_text(
        &self,
        out: &mut String,
        object: usize,
        place: Place,
        f: &Function,
        pad: &str,
    ) {
        let params: Vec<String> = f
            .params
            .iter()
            .map(|(n, t)| format!("{} {}", self.t(*t), self.t(*n)))
            .collect();
        let mut marks = String::new();
        if f.flags & FUNCTION_GLOBAL != 0 {
            marks.push_str(" global");
        }
        if f.flags & FUNCTION_NATIVE != 0 {
            marks.push_str(" native");
        }
        let name = match place {
            Place::State(_, n) => self.t(n),
            Place::Read(_) => "<read>".into(),
            Place::Write(_) => "<write>".into(),
        };
        let _ = writeln!(
            out,
            "{pad}function {name}({}) -> {}{marks}{}",
            params.join(", "),
            self.t(f.return_type),
            self.flag_names(f.user_flags)
        );
        if !self.str(f.doc).is_empty() {
            let _ = writeln!(out, "{pad}  ; doc {:?}", self.t(f.doc));
        }
        for (n, t) in &f.locals {
            let _ = writeln!(out, "{pad}  local {} {}", self.t(*t), self.t(*n));
        }
        let lines = self.debug_lines(object, place, f);
        for (k, i) in f.code.iter().enumerate() {
            let mut args: Vec<String> = i.args.iter().map(|v| self.value(v)).collect();
            if let Some(s) = target_slot(i.op) {
                if let (Some(a), Some(Value::Int(d))) = (args.get_mut(s), i.args.get(s)) {
                    *a = format!("{d:+} (-> {:04})", k as i64 + i64::from(*d));
                }
            }
            let mut line = format!("{pad}  {k:04}  {:<14} {}", op_name(i.op), args.join(", "));
            if !i.extra.is_empty() {
                let extra: Vec<String> = i.extra.iter().map(|v| self.value(v)).collect();
                let _ = write!(line, " ({})", extra.join(", "));
            }
            if let Some(n) = lines.and_then(|l| l.get(k)) {
                let _ = write!(line, "    ; line {n}");
            }
            let _ = writeln!(out, "{}", line.trim_end());
        }
    }
}

/// The whole file as text.
pub fn disassemble(p: &Pex<'_>) -> String {
    let mut out = String::new();
    let h = &p.header;
    let _ = writeln!(
        out,
        "; version {}.{} game {} compiled {}",
        h.major, h.minor, h.game, h.compiled
    );
    let _ = writeln!(
        out,
        "; source {:?} user {:?} machine {:?}",
        text(h.source),
        text(h.user),
        text(h.machine)
    );
    let s = &p.sections;
    let _ = writeln!(
        out,
        "; strings {} at {}, debug at {}, user flags at {}, objects at {}, end {}",
        p.strings.len(),
        s.strings,
        s.debug,
        s.user_flags,
        s.objects,
        s.end
    );
    match &p.debug {
        Some(d) => {
            let _ = writeln!(
                out,
                "; debug modified {}: {} function entries, {} property groups, {} struct orders",
                d.modified,
                d.functions.len(),
                d.property_groups.len(),
                d.struct_orders.len()
            );
        }
        None => {
            let _ = writeln!(out, "; no debug");
        }
    }
    let flags: Vec<String> = p
        .user_flags
        .iter()
        .map(|u| format!("{}={}", p.t(u.name), u.bit))
        .collect();
    let _ = writeln!(out, "; user flags {}", flags.join(" "));
    for (oi, o) in p.objects.iter().enumerate() {
        let _ = writeln!(
            out,
            "\nobject {} extends {:?}{} const={} auto_state={:?} size={}",
            p.t(o.name),
            p.t(o.parent),
            p.flag_names(o.user_flags),
            o.constant,
            p.t(o.auto_state),
            o.size
        );
        if !p.str(o.doc).is_empty() {
            let _ = writeln!(out, "  ; doc {:?}", p.t(o.doc));
        }
        for st in &o.structs {
            let _ = writeln!(out, "  struct {}", p.t(st.name));
            for m in &st.members {
                let _ = writeln!(
                    out,
                    "    {} {} = {}{} const={}",
                    p.t(m.type_name),
                    p.t(m.name),
                    p.value(&m.value),
                    p.flag_names(m.user_flags),
                    m.constant
                );
            }
        }
        for v in &o.variables {
            let _ = writeln!(
                out,
                "  variable {} {} = {}{} const={}",
                p.t(v.type_name),
                p.t(v.name),
                p.value(&v.value),
                p.flag_names(v.user_flags),
                v.constant
            );
        }
        for pr in &o.properties {
            let backing = if pr.flags & PROPERTY_VARIABLE != 0 {
                format!(
                    " variable {}",
                    pr.variable.map(|v| p.t(v)).unwrap_or_default()
                )
            } else {
                String::new()
            };
            let _ = writeln!(
                out,
                "  property {} {} flags={}{}{}",
                p.t(pr.type_name),
                p.t(pr.name),
                pr.flags,
                backing,
                p.flag_names(pr.user_flags)
            );
            if let Some(f) = &pr.read {
                p.function_text(&mut out, oi, Place::Read(pr.name), f, "    ");
            }
            if let Some(f) = &pr.write {
                p.function_text(&mut out, oi, Place::Write(pr.name), f, "    ");
            }
        }
        for st in &o.states {
            let _ = writeln!(out, "  state {:?}", p.t(st.name));
            for f in &st.functions {
                p.function_text(
                    &mut out,
                    oi,
                    Place::State(st.name, f.name),
                    &f.function,
                    "    ",
                );
            }
        }
    }
    out
}
