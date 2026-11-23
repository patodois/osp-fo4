//! The generator classes of a behaviour graph below its root machine, its
//! variable bindings and its variables' stored initial values, read as far
//! as following a graph from its root to a clip needs
//! (`spec/havok-behavior.md`, Generators, bindings and variables).
//!
//! Every offset was measured over the 450 behaviour graph files of one
//! installation by `char_start_clip_census` (the checks are counted in
//! `spec/character-start-clip-census.txt`); the class names are the strings
//! stored in `__classnames__`, the field labels are this project's. Fields
//! not listed are not read. What a game does with a field (which child of
//! a blend it shows, when it changes a variable) is not read here.
use super::{StateInfo, StateMachine, MAX_ENTRIES};
use crate::view::View;
use crate::Container;
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

pub const MODIFIER_GENERATOR: &str = "hkbModifierGenerator";
pub const STATE_TAGGING: &str = "BSiStateTaggingGenerator";
pub const DYNAMIC_TAGGING: &str = "DynamicAnimationTaggingGenerator";
pub const OFFSET_GENERATOR: &str = "BSOffsetAnimationGenerator";
pub const BLENDER: &str = "hkbBlenderGenerator";
pub const POSE_MATCHING: &str = "hkbPoseMatchingGenerator";
pub const BLENDER_CHILD: &str = "hkbBlenderGeneratorChild";
pub const MANUAL_SELECTOR: &str = "hkbManualSelectorGenerator";
pub const BEHAVIOR_REFERENCE: &str = "hkbBehaviorReferenceGenerator";
pub const SWAP_GENERATOR: &str = "BSBehaviorGraphSwapGenerator";
pub const CLIP_GENERATOR: &str = "hkbClipGenerator";
pub const BINDING_SET: &str = "hkbVariableBindingSet";
pub const VALUE_SET: &str = "hkbVariableValueSet";
/// Bytes of one binding element of a [`BINDING_SET`].
pub const BINDING_ELEMENT: u32 = 40;
/// Bytes of one variable information element of the graph data.
pub const VARIABLE_INFO: u32 = 6;
/// The classes whose one generator link is at +144 ([`Node::Wrapper`]).
pub const WRAPPERS: [&str; 3] = [MODIFIER_GENERATOR, STATE_TAGGING, DYNAMIC_TAGGING];
/// The classes read as [`Node::Blender`].
pub const BLENDERS: [&str; 2] = [BLENDER, POSE_MATCHING];

/// The text at +56 of a node (its name in the graph), when one is stored.
pub fn node_name<'a>(c: &Container<'a>, index: usize, file: &str, base: u64) -> Option<&'a [u8]> {
    let class = c.objects.get(index)?.class;
    let v = View::open(c, index, file, base, &[class], "InvalidHavokNode").ok()?;
    v.pointer(56)?;
    v.string(56, "node name").ok()
}

/// One element of a [`BINDING_SET`]: the member path text (+0), the
/// variable index (i32 at +28), the bit (i8 at +32, -1 for none) and the
/// binding kind (u8 at +33: 0 for a graph variable in all 4,960 bindings
/// whose index lies among the graph's variables, 1 in the other 1,534).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VariableBinding {
    pub path: String,
    pub variable: i32,
    pub bit: i8,
    pub kind: u8,
}

/// The bindings of the node `index`: the [`BINDING_SET`] linked at +16, its
/// array at +16 of [`BINDING_ELEMENT`]-byte elements; none without the link.
pub fn bindings(
    c: &Container<'_>,
    index: usize,
    file: &str,
    base: u64,
    b: &Budget,
) -> Result<Vec<VariableBinding>> {
    let class = c.objects.get(index).map_or("none", |o| o.class);
    let node = View::open(c, index, file, base, &[class], "InvalidHavokNode")?;
    let Some(set) = node.link(16) else {
        return Ok(Vec::new());
    };
    let v = View::open(c, set, file, base, &[BINDING_SET], "InvalidHavokBindingSet")?;
    let a = v.array(16, BINDING_ELEMENT)?;
    if a.count > MAX_ENTRIES {
        return Err(v.err(24, format!("at most {MAX_ENTRIES} bindings"), a.count));
    }
    let mut out = Vec::new();
    b.reserve(&mut out, a.count as usize, file, v.abs(16))?;
    for i in 0..a.count as usize {
        let e = a.element_at(i).ok_or_else(|| v.err(16, "element", i))?;
        out.push(VariableBinding {
            path: String::from_utf8_lossy(v.string(e, "binding member path")?).into_owned(),
            variable: v.u32(e + 28)? as i32,
            bit: v.u8(e + 32)? as i8,
            kind: v.u8(e + 33)?,
        });
    }
    Ok(out)
}

/// One child of a blender: the [`BLENDER_CHILD`] object, its generator
/// (link at +48) and its stored weight (f32 at +64).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BlendChild {
    pub object: usize,
    pub generator: Option<usize>,
    pub weight: f32,
}

/// A clip generator: the clip's text (+144), the trigger array link (+152)
/// and the words after it: crop start and end (f32 at +164, +168), start
/// time (+172), playback speed (+176), enforced duration (+180), user time
/// fraction (+184), binding index (i16 at +188), mode (u8 at +190: 0, 1 or
/// 2 in all 4,505 generators) and flags (u8 at +191).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClipGenerator {
    pub clip: String,
    pub triggers: Option<usize>,
    pub crop_start: f32,
    pub crop_end: f32,
    pub start_time: f32,
    pub playback_speed: f32,
    pub enforced_duration: f32,
    pub user_fraction: f32,
    pub binding_index: i16,
    pub mode: u8,
    pub flags: u8,
}

/// One generator of a graph, by class.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Node {
    /// [`STATE_MACHINE`](super::STATE_MACHINE): its start state, states and wildcards.
    Machine(StateMachine),
    /// One of [`WRAPPERS`]: the generator it wraps (link at +144).
    Wrapper { generator: Option<usize> },
    /// [`OFFSET_GENERATOR`]: a generator (link at +144) and a clip generator
    /// (link at +160).
    Offset {
        generator: Option<usize>,
        offset: Option<usize>,
    },
    /// One of [`BLENDERS`]: the children array at +160 (8-byte links to
    /// [`BLENDER_CHILD`] objects), the i16 at +152 and the u16 flag word at
    /// +154, kept as stored.
    Blender {
        word152: i16,
        flags: u16,
        children: Vec<BlendChild>,
    },
    /// [`MANUAL_SELECTOR`]: the generators array at +136 (8-byte links) and
    /// the stored index (i8 at +152).
    Selector {
        generators: Vec<usize>,
        selected: i8,
    },
    /// [`BEHAVIOR_REFERENCE`]: the behaviour file text at +136.
    Reference { behavior: String },
    /// [`SWAP_GENERATOR`]: no link is stored in any of the 47.
    Swap,
    /// [`CLIP_GENERATOR`].
    Clip(ClipGenerator),
    /// Any other class: not read.
    Unread { class: String },
}

fn links(v: &View<'_, '_>, field: usize, what: &str, file: &str, b: &Budget) -> Result<Vec<usize>> {
    let a = v.array(field, 8)?;
    if a.count > MAX_ENTRIES {
        return Err(v.err(field + 8, format!("at most {MAX_ENTRIES} {what}"), a.count));
    }
    let mut out = Vec::new();
    b.reserve(&mut out, a.count as usize, file, v.abs(field))?;
    for i in 0..a.count as usize {
        let e = a.element_at(i).ok_or_else(|| v.err(field, "element", i))?;
        out.push(v.required_link(e, what)?);
    }
    Ok(out)
}

fn f32_at(v: &View<'_, '_>, field: usize) -> Result<f32> {
    Ok(f32::from_bits(v.u32(field)?))
}

impl Node {
    /// Reads the generator object `index` by its class.
    pub fn read(
        c: &Container<'_>,
        index: usize,
        file: &str,
        base: u64,
        b: &Budget,
    ) -> Result<Self> {
        let class = c.objects.get(index).map_or("none", |o| o.class);
        let open = |code| View::open(c, index, file, base, &[class], code);
        Ok(match class {
            super::STATE_MACHINE => Node::Machine(StateMachine::read(c, index, file, base, b)?),
            x if WRAPPERS.contains(&x) => Node::Wrapper {
                generator: open("InvalidHavokNode")?.link(144),
            },
            OFFSET_GENERATOR => {
                let v = open("InvalidHavokNode")?;
                Node::Offset {
                    generator: v.link(144),
                    offset: v.link(160),
                }
            }
            x if BLENDERS.contains(&x) => {
                let v = open("InvalidHavokBlender")?;
                let objects = links(&v, 160, "blender child", file, b)?;
                let mut children = Vec::new();
                b.reserve(&mut children, objects.len(), file, v.abs(160))?;
                for o in objects {
                    let cv = View::open(c, o, file, base, &[BLENDER_CHILD], "InvalidHavokBlender")?;
                    children.push(BlendChild {
                        object: o,
                        generator: cv.link(48),
                        weight: cv.finite::<1>(64, "blend weight")?[0],
                    });
                }
                Node::Blender {
                    word152: v.u16(152)? as i16,
                    flags: v.u16(154)?,
                    children,
                }
            }
            MANUAL_SELECTOR => {
                let v = open("InvalidHavokSelector")?;
                Node::Selector {
                    generators: links(&v, 136, "selected generator", file, b)?,
                    selected: v.u8(152)? as i8,
                }
            }
            BEHAVIOR_REFERENCE => {
                let v = open("InvalidHavokReference")?;
                Node::Reference {
                    behavior: String::from_utf8_lossy(v.string(136, "behaviour file")?)
                        .into_owned(),
                }
            }
            SWAP_GENERATOR => Node::Swap,
            CLIP_GENERATOR => {
                let v = open("InvalidHavokClipGenerator")?;
                Node::Clip(ClipGenerator {
                    clip: String::from_utf8_lossy(v.string(144, "clip")?).into_owned(),
                    triggers: v.link(152),
                    crop_start: f32_at(&v, 164)?,
                    crop_end: f32_at(&v, 168)?,
                    start_time: f32_at(&v, 172)?,
                    playback_speed: f32_at(&v, 176)?,
                    enforced_duration: f32_at(&v, 180)?,
                    user_fraction: f32_at(&v, 184)?,
                    binding_index: v.u16(188)? as i16,
                    mode: v.u8(190)?,
                    flags: v.u8(191)?,
                })
            }
            other => Node::Unread {
                class: other.to_string(),
            },
        })
    }
}

/// The state of a machine whose id is `id`, read, or `None`.
pub fn machine_state<'a>(
    c: &Container<'a>,
    m: &StateMachine,
    id: i32,
    file: &str,
    base: u64,
) -> Result<Option<StateInfo<'a>>> {
    for &s in &m.states {
        let si = StateInfo::read(c, s, file, base)?;
        if si.id == id {
            return Ok(Some(si));
        }
    }
    Ok(None)
}

/// A graph's variables: each one's type byte (+4 of its 6-byte element in
/// the graph data's array at +32), its stored initial word (the value set
/// linked at +96, its word array at +16) and its name (the string data
/// linked at +104, its text array at +48). The three counts agree in all
/// 450 graph files; a file where they do not is a named error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Variables {
    pub types: Vec<u8>,
    pub words: Vec<u32>,
    pub names: Vec<String>,
    /// Entries of the value set's 16-byte array at +32 and of its link array at +48.
    pub quads: u32,
    pub variants: u32,
}
impl Variables {
    /// The variables of the graph data object `index`.
    pub fn read(
        c: &Container<'_>,
        index: usize,
        file: &str,
        base: u64,
        b: &Budget,
    ) -> Result<Self> {
        let d = View::open(
            c,
            index,
            file,
            base,
            &[super::GRAPH_DATA],
            "InvalidHavokGraphData",
        )?;
        let infos = d.array(32, VARIABLE_INFO)?;
        if infos.count > MAX_ENTRIES {
            return Err(d.err(40, format!("at most {MAX_ENTRIES} variables"), infos.count));
        }
        let mut types = Vec::new();
        b.reserve(&mut types, infos.count as usize, file, d.abs(32))?;
        for i in 0..infos.count as usize {
            let e = infos.element_at(i).ok_or_else(|| d.err(32, "element", i))?;
            types.push(d.u8(e + 4)?);
        }
        let values = d.required_link(96, "initial values")?;
        let vs = View::open(c, values, file, base, &[VALUE_SET], "InvalidHavokGraphData")?;
        let w = vs.array(16, 4)?;
        if w.count != infos.count {
            return Err(vs.err(
                24,
                format!("{} words (one per variable)", infos.count),
                w.count,
            ));
        }
        let mut words = Vec::new();
        b.reserve(&mut words, w.count as usize, file, vs.abs(16))?;
        for i in 0..w.count as usize {
            let e = w.element_at(i).ok_or_else(|| vs.err(16, "element", i))?;
            words.push(vs.u32(e)?);
        }
        let quads = vs.array(32, 16)?.count;
        let variants = vs.array(48, 8)?.count;
        let strings = d.required_link(104, "string data")?;
        let sv = View::open(
            c,
            strings,
            file,
            base,
            &[super::GRAPH_STRINGS],
            "InvalidHavokGraphData",
        )?;
        let n = sv.array(48, 8)?;
        if n.count != infos.count {
            return Err(sv.err(
                56,
                format!("{} names (one per variable)", infos.count),
                n.count,
            ));
        }
        let mut names = Vec::new();
        b.reserve(&mut names, n.count as usize, file, sv.abs(48))?;
        for i in 0..n.count as usize {
            let e = n.element_at(i).ok_or_else(|| sv.err(48, "element", i))?;
            names.push(String::from_utf8_lossy(sv.string(e, "variable name")?).into_owned());
        }
        Ok(Self {
            types,
            words,
            names,
            quads,
            variants,
        })
    }
    /// The stored initial word of variable `k` read as a number by its
    /// type byte: 0 to 3 as a signed integer of 1, 1, 2 and 4 bytes, 4 as
    /// an f32; `None` for another type or an index out of range.
    pub fn number(&self, k: i32) -> Option<f64> {
        let k = usize::try_from(k).ok()?;
        let w = *self.words.get(k)?;
        Some(match self.types.get(k)? {
            0 | 1 => f64::from(w as u8 as i8),
            2 => f64::from(w as u16 as i16),
            3 => f64::from(w as i32),
            4 => f64::from(f32::from_bits(w)),
            _ => return None,
        })
    }
}

/// The graph data object of a file's one behaviour graph.
pub fn graph_data(c: &Container<'_>, file: &str, base: u64) -> Result<usize> {
    let g = super::only_object(c, super::BEHAVIOR_GRAPH).ok_or_else(|| {
        Error::new(
            "InvalidHavokBehaviorGraph",
            file,
            base,
            "one behaviour graph",
            "none",
        )
    })?;
    super::BehaviorGraph::read(c, g, file, base)?
        .data
        .ok_or_else(|| {
            Error::new(
                "InvalidHavokBehaviorGraph",
                file,
                base,
                "a graph data link",
                "none",
            )
        })
}
