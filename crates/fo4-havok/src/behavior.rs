//! Objects of the behaviour project, character and graph `.hkx` files, read
//! as far as a state machine of Gamebryo sequence states needs
//! (`spec/havok-behavior.md`): the project's character file names, the
//! character's behaviour file name, the graph's root generator and string
//! data (its event names), a state machine's start state, states and
//! wildcard transitions, a state's id, name, generator and transitions, a
//! transition's event, target state, effect and condition links, and a
//! Gamebryo sequence generator's sequence name.
//!
//! Every offset here was measured over the 450 behaviour graph files of one
//! installation by `anim_event_census` (counts in the spec); the class names
//! are the strings stored in `__classnames__`, the field labels are this
//! project's. What the game does with a graph (which transition it takes,
//! when a sequence's text keys become events) is not read here: those are
//! pending rules of the consumers.
use crate::view::{owned_text, View};
use crate::Container;
use fo4_core::{Budget, Result};
use serde::Serialize;

pub const PROJECT_STRINGS: &str = "hkbProjectStringData";
pub const CHARACTER_STRINGS: &str = "hkbCharacterStringData";
pub const BEHAVIOR_GRAPH: &str = "hkbBehaviorGraph";
pub const GRAPH_DATA: &str = "hkbBehaviorGraphData";
pub const GRAPH_STRINGS: &str = "hkbBehaviorGraphStringData";
pub const STATE_MACHINE: &str = "hkbStateMachine";
pub const STATE_INFO: &str = "hkbStateMachineStateInfo";
pub const TRANSITIONS: &str = "hkbStateMachineTransitionInfoArray";
pub const SEQUENCE_GENERATOR: &str = "BGSGamebryoSequenceGenerator";
/// Bytes of one transition element.
pub const TRANSITION: u32 = 72;
/// The most states, transitions and event names one machine is read with.
pub const MAX_ENTRIES: u32 = 65_536;

/// The index of the one object of `class` in a file, or `None` when there
/// is none or more than one.
pub fn only_object(c: &Container<'_>, class: &str) -> Option<usize> {
    let mut found = c
        .objects
        .iter()
        .enumerate()
        .filter(|(_, o)| o.class == class);
    let first = found.next()?.0;
    found.next().is_none().then_some(first)
}

fn texts<'a>(
    v: &View<'a, '_>,
    field: usize,
    what: &str,
    file: &str,
    b: &Budget,
) -> Result<Vec<&'a [u8]>> {
    let a = v.array(field, 8)?;
    if a.count > MAX_ENTRIES {
        return Err(v.err(field + 8, format!("at most {MAX_ENTRIES} {what}"), a.count));
    }
    let mut out = Vec::new();
    b.reserve(&mut out, a.count as usize, file, v.abs(field))?;
    for i in 0..a.count as usize {
        let e = a.element_at(i).ok_or_else(|| v.err(field, "element", i))?;
        out.push(v.string(e, what)?);
    }
    Ok(out)
}

/// `hkbProjectStringData`: the array of texts at +48 (the character files,
/// relative to the project file's folder).
pub fn project_characters<'a>(
    c: &Container<'a>,
    index: usize,
    file: &str,
    base: u64,
    b: &Budget,
) -> Result<Vec<&'a [u8]>> {
    let v = View::open(
        c,
        index,
        file,
        base,
        &[PROJECT_STRINGS],
        "InvalidHavokProject",
    )?;
    texts(&v, 48, "character file", file, b)
}

/// `hkbCharacterStringData`: the text at +184 (the behaviour file, relative
/// to the project file's folder).
pub fn character_behavior<'a>(
    c: &Container<'a>,
    index: usize,
    file: &str,
    base: u64,
) -> Result<&'a [u8]> {
    let v = View::open(
        c,
        index,
        file,
        base,
        &[CHARACTER_STRINGS],
        "InvalidHavokCharacter",
    )?;
    v.string(184, "behaviour file")
}

/// `hkbBehaviorGraph`: the root generator linked from +192 and the graph
/// data linked from +200.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BehaviorGraph {
    pub object: usize,
    pub root: Option<usize>,
    pub data: Option<usize>,
}
impl BehaviorGraph {
    pub fn read(c: &Container<'_>, index: usize, file: &str, base: u64) -> Result<Self> {
        let v = View::open(
            c,
            index,
            file,
            base,
            &[BEHAVIOR_GRAPH],
            "InvalidHavokBehaviorGraph",
        )?;
        Ok(Self {
            object: index,
            root: v.link(192),
            data: v.link(200),
        })
    }
}

/// `hkbBehaviorGraphData`: the string data linked from +104.
pub fn graph_strings_link(
    c: &Container<'_>,
    index: usize,
    file: &str,
    base: u64,
) -> Result<Option<usize>> {
    let v = View::open(
        c,
        index,
        file,
        base,
        &[GRAPH_DATA],
        "InvalidHavokBehaviorGraph",
    )?;
    Ok(v.link(104))
}

/// `hkbBehaviorGraphStringData`: the array of texts at +16, the graph's
/// event names by index.
pub fn graph_events<'a>(
    c: &Container<'a>,
    index: usize,
    file: &str,
    base: u64,
    b: &Budget,
) -> Result<Vec<&'a [u8]>> {
    let v = View::open(
        c,
        index,
        file,
        base,
        &[GRAPH_STRINGS],
        "InvalidHavokBehaviorGraph",
    )?;
    texts(&v, 16, "event name", file, b)
}

/// `hkbStateMachine`: the start state id (i32 at +168), the states (array
/// of links at +208, each to a state info) and the wildcard transitions
/// (link at +224).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StateMachine {
    pub object: usize,
    pub start: i32,
    pub states: Vec<usize>,
    pub wildcards: Option<usize>,
}
impl StateMachine {
    pub fn read(
        c: &Container<'_>,
        index: usize,
        file: &str,
        base: u64,
        b: &Budget,
    ) -> Result<Self> {
        let v = View::open(
            c,
            index,
            file,
            base,
            &[STATE_MACHINE],
            "InvalidHavokStateMachine",
        )?;
        let a = v.array(208, 8)?;
        if a.count > MAX_ENTRIES {
            return Err(v.err(216, format!("at most {MAX_ENTRIES} states"), a.count));
        }
        let mut states = Vec::new();
        b.reserve(&mut states, a.count as usize, file, v.abs(208))?;
        for i in 0..a.count as usize {
            let e = a.element_at(i).ok_or_else(|| v.err(208, "element", i))?;
            let o = v.required_link(e, "state")?;
            let class = c.objects.get(o).map_or("none", |x| x.class);
            if class != STATE_INFO {
                return Err(v.err(e, STATE_INFO, class));
            }
            states.push(o);
        }
        let wildcards = v.link(224);
        if let Some(w) = wildcards {
            let class = c.objects.get(w).map_or("none", |x| x.class);
            if class != TRANSITIONS {
                return Err(v.err(224, TRANSITIONS, class));
            }
        }
        Ok(Self {
            object: index,
            start: v.u32(168)? as i32,
            states,
            wildcards,
        })
    }
}

/// `hkbStateMachineStateInfo`: the enter and exit notification links (+64,
/// +72), the transitions (+80), the generator (+88), the name (+96) and the
/// state id (i32 at +104).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StateInfo<'a> {
    pub object: usize,
    pub id: i32,
    pub name: &'a [u8],
    pub enter_notify: Option<usize>,
    pub exit_notify: Option<usize>,
    pub transitions: Option<usize>,
    pub generator: Option<usize>,
}
impl<'a> StateInfo<'a> {
    pub fn read(c: &Container<'a>, index: usize, file: &str, base: u64) -> Result<Self> {
        let v = View::open(c, index, file, base, &[STATE_INFO], "InvalidHavokState")?;
        let transitions = v.link(80);
        if let Some(t) = transitions {
            let class = c.objects.get(t).map_or("none", |x| x.class);
            if class != TRANSITIONS {
                return Err(v.err(80, TRANSITIONS, class));
            }
        }
        Ok(Self {
            object: index,
            id: v.u32(104)? as i32,
            name: v.string(96, "state name")?,
            enter_notify: v.link(64),
            exit_notify: v.link(72),
            transitions,
            generator: v.link(88),
        })
    }
}

/// One 72-byte transition element: the effect (link at +32), the condition
/// (link at +40), the event index (i32 at +48), the target state id (i32 at
/// +52) and the four words at +56..+72, kept as stored (not read).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Transition {
    pub effect: Option<usize>,
    pub condition: Option<usize>,
    pub event: i32,
    pub to_state: i32,
    pub words: [u32; 4],
}

/// `hkbStateMachineTransitionInfoArray`: the array at +16 of
/// [`TRANSITION`]-byte elements.
pub fn transitions(
    c: &Container<'_>,
    index: usize,
    file: &str,
    base: u64,
    b: &Budget,
) -> Result<Vec<Transition>> {
    let v = View::open(
        c,
        index,
        file,
        base,
        &[TRANSITIONS],
        "InvalidHavokTransitions",
    )?;
    let a = v.array(16, TRANSITION)?;
    if a.count > MAX_ENTRIES {
        return Err(v.err(24, format!("at most {MAX_ENTRIES} transitions"), a.count));
    }
    let mut out = Vec::new();
    b.reserve(&mut out, a.count as usize, file, v.abs(16))?;
    for i in 0..a.count as usize {
        let e = a.element_at(i).ok_or_else(|| v.err(16, "element", i))?;
        out.push(Transition {
            effect: v.link(e + 32),
            condition: v.link(e + 40),
            event: v.u32(e + 48)? as i32,
            to_state: v.u32(e + 52)? as i32,
            words: [
                v.u32(e + 56)?,
                v.u32(e + 60)?,
                v.u32(e + 64)?,
                v.u32(e + 68)?,
            ],
        });
    }
    Ok(out)
}

/// `BGSGamebryoSequenceGenerator`: the text at +136, the name of the NIF
/// controller sequence it plays.
pub fn sequence_name<'a>(
    c: &Container<'a>,
    index: usize,
    file: &str,
    base: u64,
) -> Result<&'a [u8]> {
    let v = View::open(
        c,
        index,
        file,
        base,
        &[SEQUENCE_GENERATOR],
        "InvalidHavokSequenceGenerator",
    )?;
    v.string(136, "sequence name")
}

/// One state of a [`Machine`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MachineState {
    pub id: i32,
    pub name: String,
    /// The generator's class (`none` without one).
    pub generator: String,
    /// The sequence a [`SEQUENCE_GENERATOR`] plays.
    pub sequence: Option<String>,
    pub transitions: Vec<Transition>,
    /// Whether the state has enter or exit notification events (not read).
    pub notify: bool,
}

/// The root state machine of a behaviour graph file, as owned values:
/// the start state, the states in stored order, the wildcard transitions
/// and the graph's event names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Machine {
    pub start: i32,
    pub states: Vec<MachineState>,
    pub wildcards: Vec<Transition>,
    pub events: Vec<String>,
}
impl Machine {
    /// Reads the graph of the file's one [`BEHAVIOR_GRAPH`] object, whose
    /// root generator must be a [`STATE_MACHINE`]. A missing object or a
    /// root of another class is a named error.
    pub fn read(c: &Container<'_>, file: &str, base: u64, b: &Budget) -> Result<Self> {
        let missing = |what: &str| {
            fo4_core::Error::new("InvalidHavokBehaviorGraph", file, base, what, "none")
        };
        let g = only_object(c, BEHAVIOR_GRAPH).ok_or_else(|| missing("one behaviour graph"))?;
        let g = BehaviorGraph::read(c, g, file, base)?;
        let data = g.data.ok_or_else(|| missing("a graph data link"))?;
        let strings = graph_strings_link(c, data, file, base)?
            .ok_or_else(|| missing("a string data link"))?;
        let events = graph_events(c, strings, file, base, b)?
            .into_iter()
            .map(|e| owned_text(e, file, base, b))
            .collect::<Result<_>>()?;
        let root = g.root.ok_or_else(|| missing("a root generator"))?;
        let class = c.objects.get(root).map_or("none", |x| x.class);
        if class != STATE_MACHINE {
            return Err(fo4_core::Error::new(
                "UnsupportedBehaviorRoot",
                file,
                base,
                STATE_MACHINE,
                class,
            ));
        }
        let sm = StateMachine::read(c, root, file, base, b)?;
        let mut states = Vec::new();
        b.reserve(&mut states, sm.states.len(), file, base)?;
        for &s in &sm.states {
            let si = StateInfo::read(c, s, file, base)?;
            let generator = si
                .generator
                .and_then(|x| c.objects.get(x))
                .map_or("none", |x| x.class)
                .to_string();
            let sequence = match si.generator {
                Some(x) if generator == SEQUENCE_GENERATOR => {
                    Some(owned_text(sequence_name(c, x, file, base)?, file, base, b)?)
                }
                _ => None,
            };
            let transitions = match si.transitions {
                Some(t) => transitions(c, t, file, base, b)?,
                None => Vec::new(),
            };
            states.push(MachineState {
                id: si.id,
                name: owned_text(si.name, file, base, b)?,
                generator,
                sequence,
                transitions,
                notify: si.enter_notify.is_some() || si.exit_notify.is_some(),
            });
        }
        let wildcards = match sm.wildcards {
            Some(w) => transitions(c, w, file, base, b)?,
            None => Vec::new(),
        };
        Ok(Self {
            start: sm.start,
            states,
            wildcards,
            events,
        })
    }
    /// The state of an id.
    pub fn state(&self, id: i32) -> Option<&MachineState> {
        self.states.iter().find(|s| s.id == id)
    }
}

mod nodes;
pub use nodes::{
    bindings, graph_data, machine_state, node_name, BlendChild, ClipGenerator, Node,
    VariableBinding, Variables, BEHAVIOR_REFERENCE, BINDING_ELEMENT, BINDING_SET, BLENDER,
    BLENDERS, BLENDER_CHILD, CLIP_GENERATOR, DYNAMIC_TAGGING, MANUAL_SELECTOR, MODIFIER_GENERATOR,
    OFFSET_GENERATOR, POSE_MATCHING, STATE_TAGGING, SWAP_GENERATOR, VALUE_SET, VARIABLE_INFO,
    WRAPPERS,
};

#[cfg(test)]
mod tests;
