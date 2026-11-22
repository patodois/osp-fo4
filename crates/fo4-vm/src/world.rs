//! What a run creates and changes: object handles, script instances,
//! arrays, structs, undetermined values and queued timers.
use std::fmt::Write as _;

pub type ObjId = usize;
pub type InstId = usize;
pub type ArrId = usize;
pub type StructId = usize;
pub type UnknownId = usize;

/// A value while a script runs.
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    None,
    Int(i32),
    Float(f32),
    Bool(bool),
    Str(String),
    Object(ObjId),
    Array(ArrId),
    Struct(StructId),
    /// A value nothing determines (a native's result, or a conversion no
    /// rule settles): it may be stored, passed and logged, and the run
    /// blocks where it would steer control flow.
    Unknown(UnknownId),
}

/// Why a value is undetermined, or why a run blocked.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    /// The result of a native function (`script.function`, lower case).
    Native(String),
    /// A conversion or comparison the named rule leaves open, with a short
    /// description.
    Rule(&'static str, String),
    /// A scripted call or property on an object that carries no instance
    /// of the type: lower-case type name, and the kind of the object
    /// ([`World::object_kind`]).
    NoInstance(String, String),
}
impl Origin {
    pub fn text(&self) -> String {
        match self {
            Origin::Native(n) => format!("native {n}"),
            Origin::Rule(r, d) => format!("rule {r}: {d}"),
            Origin::NoInstance(t, k) => format!("no script instance of {t} ({k})"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Object {
    /// Printed in the effect log.
    pub label: String,
    /// Identity of a record (for example a FormID resolved through the load
    /// order); two objects with different keys are different objects
    /// (`vm_object_identity_v1`). `None` for opaque handles such as the
    /// player's.
    pub key: Option<String>,
    /// Lower-case types the object is known to be (each with its parents).
    pub types: Vec<String>,
    /// Script instances attached to the object, in attachment order.
    pub instances: Vec<InstId>,
    /// The engine type of the record behind the object by its record type
    /// (`vm_record_signature_type_v1`), when known.
    pub signature_type: Option<String>,
    /// Whether the scripts of the record behind the object were attached
    /// (or the object was bound), so that [`crate::Attach`] is asked once.
    pub attached: bool,
}

#[derive(Debug, Clone)]
pub struct VarSlot {
    pub script: String,
    pub name: String,
    pub type_name: String,
    pub value: Val,
}

#[derive(Debug, Clone)]
pub struct Instance {
    pub object: Option<ObjId>,
    /// The attached (most derived) script, lower case.
    pub script: String,
    /// Current state, lower case (`pex_state_identifier_v1`).
    pub state: String,
    /// Variables of the script and of every parent.
    pub vars: Vec<VarSlot>,
}

#[derive(Debug, Clone)]
pub struct Array {
    /// Lower-case element type.
    pub elem: String,
    pub items: Vec<Val>,
}

#[derive(Debug, Clone)]
pub struct StructVal {
    /// `script#struct`, lower case.
    pub type_name: String,
    /// (member, type, value).
    pub members: Vec<(String, String, Val)>,
}

#[derive(Debug, Clone)]
pub struct Unknown {
    pub origin: Origin,
}

/// Which clock a timer counts on (`vm_timer_events_v1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clock {
    Real,
    GameTime,
}

#[derive(Debug, Clone)]
pub struct Timer {
    pub clock: Clock,
    /// Due time on its clock; `None` when the interval was undetermined.
    pub due: Option<f64>,
    pub seq: u64,
    pub instance: InstId,
    pub id: Val,
}

#[derive(Debug, Default)]
pub struct World {
    pub objects: Vec<Object>,
    pub instances: Vec<Instance>,
    pub arrays: Vec<Array>,
    pub structs: Vec<StructVal>,
    pub unknowns: Vec<Unknown>,
    pub timers: Vec<Timer>,
    pub timer_seq: u64,
    /// Time already elapsed on each clock by fired timers.
    pub now: [f64; 2],
    /// The player handle (`vm_host_player_handle_v1`), once created.
    pub player: Option<ObjId>,
    /// The player's handle is different from every record's handle
    /// (`vm_player_no_record_v1`; `quest_run --player-no-record`). Off by
    /// default: such a comparison is left open, as before.
    pub player_no_record: bool,
    /// Objects standing for one placement of a base (the base census),
    /// with the base's record key.
    pub placement_bases: Vec<(ObjId, String)>,
    /// Quest state and the stage fragments and quest events queued
    /// (`crate::quest`).
    pub quests: crate::quest::Quests,
    /// Remote and custom event registrations (`crate::events`).
    pub events: crate::events::Events,
    /// Scene state and the steps of playing scenes (`crate::scene`).
    pub scenes: crate::scene::Scenes,
    /// The player's position, trigger volumes and distance registrations
    /// (`crate::track`); off unless a caller gives a track.
    pub place: crate::track::Place,
    /// Dialogue outside scenes (`crate::talk`); off unless a caller turns
    /// a switch on.
    pub talk: crate::talk::Talk,
    /// Animation event registrations and the graphs of references
    /// (`crate::anim`); off unless a caller turns a switch on.
    pub anims: crate::anim::Anims,
}

impl World {
    pub fn object(
        &mut self,
        label: impl Into<String>,
        key: Option<String>,
        types: &[&str],
    ) -> ObjId {
        self.objects.push(Object {
            label: label.into(),
            key,
            types: types.iter().map(|t| t.to_ascii_lowercase()).collect(),
            instances: Vec::new(),
            signature_type: None,
            attached: false,
        });
        self.objects.len() - 1
    }
    /// The object with that key, created when absent.
    pub fn keyed_object(&mut self, label: &str, key: &str, type_name: &str) -> ObjId {
        let t = type_name.to_ascii_lowercase();
        if let Some(i) = self
            .objects
            .iter()
            .position(|o| o.key.as_deref() == Some(key))
        {
            if !self.objects[i].types.contains(&t) {
                self.objects[i].types.push(t);
            }
            return i;
        }
        self.object(label, Some(key.to_string()), &[type_name])
    }
    /// The player handle: one opaque object, created on first use, known
    /// to be of the given type (`vm_host_player_handle_v1`).
    pub fn player(&mut self, type_name: &str) -> ObjId {
        let o = match self.player {
            Some(o) => o,
            None => {
                let o = self.object(crate::host::PLAYER_LABEL, None, &[]);
                self.player = Some(o);
                o
            }
        };
        let t = type_name.to_ascii_lowercase();
        if !self.objects[o].types.contains(&t) {
            self.objects[o].types.push(t);
        }
        o
    }
    /// What kind of object a handle is, for counts: `player`, `alias of a
    /// quest` (key with `#alias:`), `record XXXX` (keyed: the label's
    /// record type), `placement of a base` or `opaque`.
    pub fn object_kind(&self, o: ObjId) -> String {
        let ob = &self.objects[o];
        if self.player == Some(o) {
            return "player".into();
        }
        match &ob.key {
            Some(k) if k.contains("#alias:") => "alias of a quest".into(),
            Some(_) => format!("record {}", ob.label.get(..4).unwrap_or("????")),
            None if ob.label.starts_with("placement of ") => "placement of a base".into(),
            None if ob.label.starts_with("alias ") => "alias (opaque)".into(),
            None => "opaque".into(),
        }
    }
    pub fn unknown(&mut self, origin: Origin) -> Val {
        self.unknowns.push(Unknown { origin });
        Val::Unknown(self.unknowns.len() - 1)
    }
    pub fn origin(&self, u: UnknownId) -> &Origin {
        &self.unknowns[u].origin
    }
    pub fn array(&mut self, elem: &str, items: Vec<Val>) -> Val {
        self.arrays.push(Array {
            elem: elem.to_string(),
            items,
        });
        Val::Array(self.arrays.len() - 1)
    }
    /// Text of a value for the effect log.
    pub fn render(&self, v: &Val) -> String {
        let mut s = String::new();
        self.render_into(&mut s, v, 0);
        s
    }
    fn render_into(&self, s: &mut String, v: &Val, depth: usize) {
        let _ = match v {
            Val::None => write!(s, "none"),
            Val::Int(n) => write!(s, "{n}"),
            Val::Float(f) => write!(s, "{f:?}"),
            Val::Bool(b) => write!(s, "{b}"),
            Val::Str(t) => write!(s, "{t:?}"),
            Val::Object(o) => write!(s, "<{}>", self.objects[*o].label),
            Val::Unknown(u) => write!(s, "?{u}({})", self.unknowns[*u].origin.text()),
            Val::Array(a) => {
                let a = &self.arrays[*a];
                let _ = write!(s, "{}[", a.elem);
                for (i, x) in a.items.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    if depth > 2 || i >= 16 {
                        s.push_str("...");
                        break;
                    }
                    self.render_into(s, x, depth + 1);
                }
                write!(s, "]")
            }
            Val::Struct(i) => {
                let st = &self.structs[*i];
                let _ = write!(s, "{}{{", st.type_name);
                for (k, (n, _, x)) in st.members.iter().enumerate() {
                    if k > 0 {
                        s.push_str(", ");
                    }
                    let _ = write!(s, "{n}: ");
                    if depth > 2 {
                        s.push_str("...");
                    } else {
                        self.render_into(s, x, depth + 1);
                    }
                }
                write!(s, "}}")
            }
        };
    }
}
