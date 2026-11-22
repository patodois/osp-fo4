//! The value a variable of a quest's scripts holds before any script
//! changes it, read from the records and the compiled scripts, for
//! condition function 629 (`cond_fn_629_quest_variable_v1`): the value
//! of the VMAD property the variable backs, when the quest's VMAD gives
//! one (`vmad_property_sets_variable_v1`); otherwise the variable's stored
//! initial value, or its type's default for none
//! (`pex_default_values_v1`). The scripts read are the quest's VMAD
//! scripts and its fragment script, each with its parents.
use crate::{
    program::{Library, Lit},
    world::Val,
};
use fo4_core::Budget;
use fo4_pex::index::{lower, MAX_PARENTS};
use fo4_plugin::{
    typed::Store,
    vmad::{Property, PropertyValue, Vmad},
    FormKey,
};

/// A value as a condition compares it: a number, a truth value as 1 or 0.
pub fn number(v: &Val) -> Result<f32, &'static str> {
    match v {
        Val::Int(n) => Ok(*n as f32),
        Val::Float(f) => Ok(*f),
        Val::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Val::Unknown(_) => Err("the variable's value is undetermined in this run"),
        _ => Err("the variable holds neither a number nor a truth value"),
    }
}

fn stored(v: &PropertyValue<'_>) -> Result<f32, &'static str> {
    match v {
        PropertyValue::Int(n) => Ok(*n as f32),
        PropertyValue::Float(f) => Ok(*f),
        PropertyValue::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        _ => Err("the property value is neither a number nor a truth value"),
    }
}

fn initial(l: &Lit, type_name: &str) -> Result<f32, &'static str> {
    match l {
        Lit::Int(n) => Ok(*n as f32),
        Lit::Float(f) => Ok(*f),
        Lit::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Lit::None => match type_name {
            "int" | "float" | "bool" => Ok(0.0),
            _ => Err("the variable holds neither a number nor a truth value"),
        },
        _ => Err("the variable holds neither a number nor a truth value"),
    }
}

/// The value of variable `name` (lower case) of the scripts `scripts`
/// (each with the VMAD properties given to it) declare: `Err` when none or
/// more than one declares it with different values.
pub fn declared(
    lib: &mut dyn Library,
    scripts: &[(String, &[Property<'_>])],
    name: &str,
) -> Result<f32, &'static str> {
    let mut found: Vec<Result<f32, &'static str>> = Vec::new();
    for (script, props) in scripts {
        let mut cur = script.clone();
        let mut depth = 0;
        while !cur.is_empty() {
            if depth > MAX_PARENTS {
                return Err("a script's parent chain is longer than followed");
            }
            depth += 1;
            let s = lib
                .load(&cur)
                .map_err(|_| "a script of the quest could not be read")?
                .ok_or("a script of the quest is not in the archives")?;
            if let Some(d) = s.variable(name) {
                let prop = s
                    .properties
                    .iter()
                    .find(|p| p.variable.as_deref() == Some(name));
                let given = prop.and_then(|p| props.iter().find(|q| lower(q.name) == p.name));
                found.push(match given {
                    Some(q) => stored(&q.value),
                    None => initial(&d.init, &d.type_name),
                });
            }
            cur = s.parent.clone();
        }
    }
    let mut it = found.into_iter();
    let first = it
        .next()
        .ok_or("no script of the quest declares the variable")??;
    for x in it {
        if x? != first {
            return Err("more than one script of the quest declares the variable");
        }
    }
    Ok(first)
}

/// The value variable `name` (lower case) of the quest's scripts holds
/// before any script runs.
pub fn quest_variable_initial(
    store: &Store,
    lib: &mut dyn Library,
    quest: &FormKey,
    name: &str,
    b: &Budget,
) -> Result<f32, &'static str> {
    let (r, _) = crate::bind::vmad_of(store, quest, b)
        .map_err(|_| "the quest record could not be read")?
        .ok_or("the quest has no VMAD")?;
    let s = r
        .subrecords
        .iter()
        .find(|s| s.signature == *b"VMAD")
        .ok_or("the quest has no VMAD")?;
    let v = Vmad::read(r.data(s), r.signature, &r.source.file, s.offset, b)
        .map_err(|_| "the quest's VMAD could not be read")?;
    let mut scripts: Vec<(String, &[Property<'_>])> = v
        .scripts
        .iter()
        .map(|x| (lower(x.name), x.properties.as_slice()))
        .collect();
    if let Some(q) = &v.quest {
        if !q.script.name.is_empty() {
            scripts.push((lower(q.script.name), q.script.properties.as_slice()));
        }
    }
    declared(lib, &scripts, name)
}
