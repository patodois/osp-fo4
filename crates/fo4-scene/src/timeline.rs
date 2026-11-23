//! A quest run's timeline: what a run of `fo4_vm` (`quest_run new-game
//! --timeline FILE`) did on its real-time clock, in a neutral, versioned
//! text format (`spec/timeline.md`), and the state it gives at one second,
//! for a frame drawn "at second S of the run".
//!
//! The format holds only what the run's world model recorded: the caller's
//! player track, actor positions and the rule or assumption each came
//! from, references enabled, disabled and moved, scene lines with their
//! seconds, stages and objectives. Reading it back assumes nothing.
//!
//! What a frame does with it is split in two:
//!
//! - behaviour read from the run, each piece under the rule it relies on:
//!   a reference the run disabled is not drawn and one it enabled is
//!   (`vm_disable_hides_reference_v1`), enable children follow
//!   (`xesp_script_change_propagates_v1`), a reference the run moved is
//!   drawn at its new place with its stored rotation
//!   (`timeline_move_stored_rotation_v1`), the player's exterior cell is the
//!   one whose grid square holds the position (`world_cell_natives_v1`),
//!   an actor's presence follows its own change or its enable chain;
//! - presentation, never game behaviour: the camera (the caller's eye
//!   height, the track's direction of movement or the caller's heading),
//!   the actors' placeholder markers, the overlay text.
use crate::{enable, hypotheses, Loader, RuleLevel, Scene};
use fo4_core::{Budget, Error};
use fo4_plugin::{typed::Store, FormKey};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

pub mod cast;

/// First word of a timeline file.
pub const MAGIC: &str = "osp-run-timeline";
/// The format version this module writes and reads.
pub const VERSION: u32 = 1;
/// Side of an exterior cell's grid square, as `world_cell_natives_v1`
/// reads it (the placed references of exterior cells measure it,
/// `spec/world-census.txt`).
pub const EXTERIOR_CELL_UNITS: f64 = 4096.0;
/// A horizontal move shorter than this (world units) gives no direction.
const MIN_MOVE: f64 = 1e-3;

/// A record key as the timeline writes it: `plugin:XXXXXXXX`, the plugin
/// lower case.
pub fn key_text(k: &FormKey) -> String {
    format!("{}:{:08X}", k.plugin.to_ascii_lowercase(), k.object_id)
}

/// Reads [`key_text`]; the plugin is kept lower case.
pub fn parse_key(s: &str) -> Option<FormKey> {
    let (p, id) = s.rsplit_once(':')?;
    if p.is_empty() || id.len() != 8 {
        return None;
    }
    Some(FormKey {
        plugin: p.to_ascii_lowercase(),
        object_id: u32::from_str_radix(id, 16).ok()?,
    })
}

/// The key with its plugin lower case, as every map of this module holds it.
pub fn norm(k: &FormKey) -> FormKey {
    FormKey {
        plugin: k.plugin.to_ascii_lowercase(),
        object_id: k.object_id,
    }
}

/// Where something stands: its space (an interior cell or a worldspace)
/// and position, or why that is not known.
#[derive(Debug, Clone, PartialEq)]
pub enum Spot {
    At { space: FormKey, pos: [f64; 3] },
    Open(String),
}

/// A spoken line as the run started it.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub scene: FormKey,
    /// The phase and the action's index in the scene record.
    pub phase: usize,
    pub action: usize,
    /// The reference the speaker alias held in the run, when the writer
    /// could tell.
    pub speaker: Option<FormKey>,
    /// How long the run timed it, when it did.
    pub seconds: Option<f64>,
    pub topic: Option<FormKey>,
    /// How the seconds and the speaker were settled, as the run says.
    pub how: String,
    /// The line's text, when the data holds it and one line plays.
    pub text: Option<String>,
}

/// One kind of objective change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ObjectiveKind {
    Displayed,
    Completed,
    Failed,
}
impl ObjectiveKind {
    fn text(self) -> &'static str {
        match self {
            Self::Displayed => "displayed",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "displayed" => Self::Displayed,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            _ => return None,
        })
    }
}

/// What happened at one second.
#[derive(Debug, Clone, PartialEq)]
pub enum What {
    /// A point of the caller's player track: the space, the cell the
    /// writer found for the point (an interior cell is its space), the
    /// position and the facing in degrees when the track gives one.
    Player {
        space: FormKey,
        cell: Option<FormKey>,
        pos: [f64; 3],
        facing: Option<f64>,
    },
    /// An actor stands here from this second; `source` names the rule or
    /// the caller assumption the run took it from.
    Actor {
        reference: FormKey,
        at: Spot,
        source: String,
    },
    /// The run enabled (`true`) or disabled a reference; `nowait` for the
    /// natives' NoWait forms, which a frame applies only under
    /// `timeline_nowait_visibility_v1` ([`Readings::nowait`]).
    Visibility {
        reference: FormKey,
        shown: bool,
        nowait: bool,
    },
    /// The run moved a reference other than an actor.
    Move {
        reference: FormKey,
        at: Spot,
        source: String,
    },
    Stage {
        quest: FormKey,
        stage: i32,
    },
    Objective {
        quest: FormKey,
        objective: i32,
        kind: ObjectiveKind,
        value: Option<bool>,
    },
    /// Another change of a quest (started, stopped, ...), as text.
    Quest {
        quest: FormKey,
        change: String,
    },
    /// A change of a scene (started, phase entered, ended, ...), as text.
    Scene {
        scene: FormKey,
        change: String,
    },
    Line(Line),
    /// Anything else the writer kept, as text (an enable of an object that
    /// is not a placed record, for example).
    Note(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Second of the run's real-time clock.
    pub t: f64,
    pub what: What,
}

/// A whole timeline: labels (editor ids, for people reading it), header
/// notes and the events, earliest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Timeline {
    pub labels: BTreeMap<FormKey, String>,
    pub notes: Vec<String>,
    pub events: Vec<Event>,
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\t' => o.push_str("\\t"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            c => o.push(c),
        }
    }
    o
}

fn unesc(s: &str) -> Result<String, String> {
    let mut o = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            o.push(c);
            continue;
        }
        match it.next() {
            Some('\\') => o.push('\\'),
            Some('t') => o.push('\t'),
            Some('n') => o.push('\n'),
            Some('r') => o.push('\r'),
            _ => return Err(format!("a bad escape in {s:?}")),
        }
    }
    Ok(o)
}

fn opt<T>(v: &Option<T>, f: impl Fn(&T) -> String) -> String {
    v.as_ref().map_or_else(|| "-".into(), f)
}

fn spot_text(s: &Spot) -> String {
    match s {
        Spot::At { space, pos } => {
            format!("{}\t{}\t{}\t{}", key_text(space), pos[0], pos[1], pos[2])
        }
        Spot::Open(why) => format!("-\t-\t-\t-\t{}", esc(why)),
    }
}

impl Timeline {
    /// Sorts the events by second, keeping the order of one second.
    pub fn sort(&mut self) {
        self.events.sort_by(|a, b| a.t.total_cmp(&b.t));
    }

    /// The file's text (`spec/timeline.md`).
    pub fn write(&self) -> String {
        let mut s = format!("{MAGIC}\t{VERSION}\n");
        for n in &self.notes {
            let _ = writeln!(s, "note\t{}", esc(n));
        }
        for (k, l) in &self.labels {
            let _ = writeln!(s, "label\t{}\t{}", key_text(k), esc(l));
        }
        for e in &self.events {
            let t = e.t;
            let _ = match &e.what {
                What::Player {
                    space,
                    cell,
                    pos,
                    facing,
                } => writeln!(
                    s,
                    "player\t{t}\t{}\t{}\t{}\t{}\t{}\t{}",
                    key_text(space),
                    opt(cell, key_text),
                    pos[0],
                    pos[1],
                    pos[2],
                    opt(facing, |f| f.to_string())
                ),
                What::Actor {
                    reference,
                    at,
                    source,
                } => writeln!(
                    s,
                    "actor\t{t}\t{}\t{}\t{}",
                    key_text(reference),
                    esc(source),
                    spot_text(at)
                ),
                What::Visibility {
                    reference,
                    shown,
                    nowait,
                } => writeln!(
                    s,
                    "visibility\t{t}\t{}\t{}\t{}",
                    key_text(reference),
                    if *shown { "enabled" } else { "disabled" },
                    if *nowait { "nowait" } else { "plain" }
                ),
                What::Move {
                    reference,
                    at,
                    source,
                } => writeln!(
                    s,
                    "move\t{t}\t{}\t{}\t{}",
                    key_text(reference),
                    esc(source),
                    spot_text(at)
                ),
                What::Stage { quest, stage } => {
                    writeln!(s, "stage\t{t}\t{}\t{stage}", key_text(quest))
                }
                What::Objective {
                    quest,
                    objective,
                    kind,
                    value,
                } => writeln!(
                    s,
                    "objective\t{t}\t{}\t{objective}\t{}\t{}",
                    key_text(quest),
                    kind.text(),
                    opt(value, |v| v.to_string())
                ),
                What::Quest { quest, change } => {
                    writeln!(s, "quest\t{t}\t{}\t{}", key_text(quest), esc(change))
                }
                What::Scene { scene, change } => {
                    writeln!(s, "scene\t{t}\t{}\t{}", key_text(scene), esc(change))
                }
                What::Line(l) => writeln!(
                    s,
                    "line\t{t}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    key_text(&l.scene),
                    l.phase,
                    l.action,
                    opt(&l.speaker, key_text),
                    opt(&l.seconds, |x| x.to_string()),
                    opt(&l.topic, key_text),
                    esc(&l.how),
                    opt(&l.text, |x| esc(x))
                ),
                What::Note(n) => writeln!(s, "event\t{t}\t{}", esc(n)),
            };
        }
        s
    }

    /// Reads [`Timeline::write`]'s text: the first line names the format
    /// and its version; an unknown kind of line, a wrong field count or a
    /// bad value is an error naming the line.
    pub fn parse(text: &str) -> Result<Timeline, String> {
        let mut lines = text.lines().enumerate();
        let head = lines.next().map(|(_, l)| l).unwrap_or("");
        match head.split_once('\t') {
            Some((m, v)) if m == MAGIC => {
                if v.parse::<u32>().ok() != Some(VERSION) {
                    return Err(format!(
                        "timeline version {v}: this reader knows version {VERSION}"
                    ));
                }
            }
            _ => return Err(format!("not a timeline: the first line is not {MAGIC}")),
        }
        let mut out = Timeline::default();
        for (n, line) in lines {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let bad = |why: &str| format!("timeline line {}: {why}", n + 1);
            let f: Vec<&str> = line.split('\t').collect();
            let want = |k: usize| -> Result<(), String> {
                if f.len() == k {
                    Ok(())
                } else {
                    Err(bad(&format!("{} fields, {k} expected", f.len())))
                }
            };
            let key = |s: &str| parse_key(s).ok_or_else(|| bad("a key as plugin:XXXXXXXX"));
            let num = |s: &str| -> Result<f64, String> {
                s.parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| bad("a finite number"))
            };
            let okey = |s: &str| -> Result<Option<FormKey>, String> {
                if s == "-" {
                    Ok(None)
                } else {
                    key(s).map(Some)
                }
            };
            let onum = |s: &str| -> Result<Option<f64>, String> {
                if s == "-" {
                    Ok(None)
                } else {
                    num(s).map(Some)
                }
            };
            let spot = |g: &[&str]| -> Result<Spot, String> {
                match g {
                    [s, x, y, z] if *s != "-" => Ok(Spot::At {
                        space: key(s)?,
                        pos: [num(x)?, num(y)?, num(z)?],
                    }),
                    ["-", "-", "-", "-", why] => Ok(Spot::Open(unesc(why)?)),
                    _ => Err(bad("a place as SPACE X Y Z or - - - - WHY")),
                }
            };
            let kind = f[0];
            match kind {
                "note" => {
                    want(2)?;
                    out.notes.push(unesc(f[1])?);
                    continue;
                }
                "label" => {
                    want(3)?;
                    out.labels.insert(key(f[1])?, unesc(f[2])?);
                    continue;
                }
                _ => {}
            }
            if f.len() < 2 {
                return Err(bad("no second"));
            }
            let t = num(f[1])?;
            let what = match kind {
                "player" => {
                    want(8)?;
                    What::Player {
                        space: key(f[2])?,
                        cell: okey(f[3])?,
                        pos: [num(f[4])?, num(f[5])?, num(f[6])?],
                        facing: onum(f[7])?,
                    }
                }
                "actor" | "move" => {
                    if f.len() != 8 && f.len() != 9 {
                        return Err(bad("8 or 9 fields"));
                    }
                    let (reference, source, at) = (key(f[2])?, unesc(f[3])?, spot(&f[4..])?);
                    if kind == "actor" {
                        What::Actor {
                            reference,
                            at,
                            source,
                        }
                    } else {
                        What::Move {
                            reference,
                            at,
                            source,
                        }
                    }
                }
                "visibility" => {
                    want(5)?;
                    What::Visibility {
                        reference: key(f[2])?,
                        shown: match f[3] {
                            "enabled" => true,
                            "disabled" => false,
                            _ => return Err(bad("enabled or disabled")),
                        },
                        nowait: match f[4] {
                            "nowait" => true,
                            "plain" => false,
                            _ => return Err(bad("nowait or plain")),
                        },
                    }
                }
                "stage" => {
                    want(4)?;
                    What::Stage {
                        quest: key(f[2])?,
                        stage: f[3].parse().map_err(|_| bad("a stage number"))?,
                    }
                }
                "objective" => {
                    want(6)?;
                    What::Objective {
                        quest: key(f[2])?,
                        objective: f[3].parse().map_err(|_| bad("an objective number"))?,
                        kind: ObjectiveKind::parse(f[4])
                            .ok_or_else(|| bad("displayed, completed or failed"))?,
                        value: match f[5] {
                            "-" => None,
                            "true" => Some(true),
                            "false" => Some(false),
                            _ => return Err(bad("true, false or -")),
                        },
                    }
                }
                "quest" => {
                    want(4)?;
                    What::Quest {
                        quest: key(f[2])?,
                        change: unesc(f[3])?,
                    }
                }
                "scene" => {
                    want(4)?;
                    What::Scene {
                        scene: key(f[2])?,
                        change: unesc(f[3])?,
                    }
                }
                "line" => {
                    want(10)?;
                    What::Line(Line {
                        scene: key(f[2])?,
                        phase: f[3].parse().map_err(|_| bad("a phase index"))?,
                        action: f[4].parse().map_err(|_| bad("an action index"))?,
                        speaker: okey(f[5])?,
                        seconds: onum(f[6])?,
                        topic: okey(f[7])?,
                        how: unesc(f[8])?,
                        text: if f[9] == "-" {
                            None
                        } else {
                            Some(unesc(f[9])?)
                        },
                    })
                }
                "event" => {
                    want(3)?;
                    What::Note(unesc(f[2])?)
                }
                other => return Err(bad(&format!("unknown kind {other:?}"))),
            };
            if out.events.last().is_some_and(|e| e.t > t) {
                return Err(bad("time goes back"));
            }
            out.events.push(Event { t, what });
        }
        Ok(out)
    }

    /// The label of a key, else its text.
    pub fn label(&self, k: &FormKey) -> String {
        match self.labels.get(&norm(k)) {
            Some(l) => format!("{l} {}", key_text(k)),
            None => key_text(k),
        }
    }

    /// The state the timeline gives at second `s` ([`Frame`]) with every
    /// reading off ([`Readings::default`]).
    pub fn at(&self, s: f64) -> Frame {
        self.at_with(s, Readings::default())
    }

    /// [`Timeline::at`] with the readings `r`.
    pub fn at_with(&self, s: f64, r: Readings) -> Frame {
        let mut f = Frame {
            second: s,
            readings: r,
            ..Frame::default()
        };
        let points: Vec<(f64, &FormKey, &Option<FormKey>, [f64; 3])> = self
            .events
            .iter()
            .filter_map(|e| match &e.what {
                What::Player {
                    space, cell, pos, ..
                } => Some((e.t, space, cell, *pos)),
                _ => None,
            })
            .collect();
        f.player = player_at(&points, s);
        for e in self.events.iter().filter(|e| e.t <= s) {
            match &e.what {
                What::Visibility {
                    reference,
                    shown,
                    nowait,
                } => {
                    if *nowait && !r.nowait {
                        f.nowait_left += 1;
                        continue;
                    }
                    if *nowait {
                        f.rules.insert(hypotheses::TIMELINE_NOWAIT);
                    }
                    f.visibility.insert(norm(reference), *shown);
                }
                What::Move {
                    reference,
                    at,
                    source,
                } => {
                    f.moves
                        .insert(norm(reference), (e.t, at.clone(), source.clone()));
                }
                What::Actor {
                    reference,
                    at,
                    source,
                } => {
                    f.actors
                        .insert(norm(reference), (e.t, at.clone(), source.clone()));
                }
                What::Stage { quest, stage } => {
                    f.stages.insert(norm(quest), (*stage, e.t));
                    f.last_stage = Some((norm(quest), *stage, e.t));
                }
                What::Objective {
                    quest,
                    objective,
                    kind,
                    value,
                } => {
                    f.objectives
                        .entry((norm(quest), *objective))
                        .or_default()
                        .insert(*kind, (*value, e.t));
                }
                What::Line(l) => {
                    f.last_line = Some((e.t, l.clone()));
                    if l.seconds.is_some_and(|d| s < e.t + d) {
                        f.spoken.push((e.t, l.clone()));
                    }
                }
                What::Note(text) => match body_note(text) {
                    Some(BodyNote::Death(Some(k))) => {
                        f.dead.entry(k).or_insert(e.t);
                    }
                    Some(BodyNote::Sit(who, furniture)) => {
                        f.sitting.insert(who, (e.t, furniture));
                    }
                    Some(BodyNote::GetUp(who, furniture))
                        if f.sitting.get(&who).is_some_and(|(_, x)| *x == furniture) =>
                    {
                        f.sitting.remove(&who);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        f
    }
}

/// The player at a second: where the track puts it (straight moves at
/// constant speed between two points of one space, a jump at the second of
/// a point in another space: the caller's track model, `fo4_vm::track`)
/// and the direction of its horizontal movement, when there is one.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerAt {
    pub space: FormKey,
    /// The cell the writer found for the track point the position comes
    /// from (in a worldspace, [`exterior_cell`] gives the one under the
    /// position itself).
    pub cell: Option<FormKey>,
    pub pos: [f64; 3],
    /// Unit direction in the XY plane, and where it was taken from.
    pub heading: Option<([f64; 2], &'static str)>,
}

/// The last value of each kind of change of one objective, with its second.
pub type ObjectiveValues = BTreeMap<ObjectiveKind, (Option<bool>, f64)>;

/// The markers of a frame, one line per actor left out, and the rules
/// relied on ([`markers`]).
pub type Markers = (Vec<Marker>, Vec<String>, BTreeSet<&'static str>);

/// Readings of a timeline a frame may take, each off by default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Readings {
    /// `timeline_nowait_visibility_v1` (`--timeline-nowait`): the NoWait
    /// forms of Enable and Disable change presence as the plain natives
    /// do. Off: they are counted ([`Frame::nowait_left`]) and change
    /// nothing.
    pub nowait: bool,
    /// `xesp_child_flag_agrees_chain_v1` (`--timeline-child-flag-agrees`):
    /// an actor's enable chain that the rules leave open only because a
    /// record below its root carries its own flag 0x800 is read as any
    /// other chain when every such flag agrees with the start state the
    /// chain gives. Off: such an actor's presence stays undecided.
    pub child_flag_agrees: bool,
    /// What a frame does with an actor the run has dead (the `death`
    /// event lines, `spec/timeline.md`, Deaths and furniture). Off: drawn
    /// as before, standing.
    pub dead: Dead,
    /// `timeline_sit_marker_nearest_v1` (`--timeline-sit-marker`): an
    /// actor the run has sitting in a furniture reference (the `sit` and
    /// `get-up` event lines) stands at the furniture's marker nearest its
    /// place, read as `furniture_marker_snam_v1` reads the furniture's
    /// markers ([`crate::furniture`]), facing the marker's heading. Off:
    /// drawn as before.
    pub sit_marker: bool,
}

/// What a frame does with a dead actor ([`Readings::dead`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Dead {
    /// Drawn as before (standing where the run last put it).
    #[default]
    Standing,
    /// `timeline_dead_actor_left_out_v1` (`--timeline-dead-left-out`):
    /// not drawn from its death on.
    LeftOut,
    /// `timeline_dead_actor_lying_placeholder_v1` (`--timeline-dead-lying`):
    /// drawn lying at its last place, its pose turned a quarter turn about
    /// its own side axis (a placeholder: the data's death clips and ragdoll
    /// are not read, `spec/timeline.md`).
    Lying,
}

/// Who a `sit` or `get-up` event line names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sitter {
    Player,
    Actor(FormKey),
}

/// A `death`, `sit` or `get-up` event line's text (`spec/timeline.md`,
/// Deaths and furniture), read back; `None` for any other text.
pub fn body_note(text: &str) -> Option<BodyNote> {
    let w: Vec<&str> = text.split(' ').collect();
    let who = |s: &str| -> Option<Sitter> {
        if s == "player" {
            Some(Sitter::Player)
        } else {
            parse_key(s).map(|k| Sitter::Actor(norm(&k)))
        }
    };
    match w.as_slice() {
        ["death", r] => Some(BodyNote::Death(if *r == "-" {
            None
        } else {
            Some(norm(&parse_key(r)?))
        })),
        ["sit", s, f] => Some(BodyNote::Sit(who(s)?, norm(&parse_key(f)?))),
        ["get-up", s, f] => Some(BodyNote::GetUp(who(s)?, norm(&parse_key(f)?))),
        _ => None,
    }
}

/// A `death`, `sit` or `get-up` event line ([`body_note`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyNote {
    /// The dead reference, `None` for `-` (no record).
    Death(Option<FormKey>),
    Sit(Sitter, FormKey),
    GetUp(Sitter, FormKey),
}

/// The state at one second ([`Timeline::at`]). Every map is keyed by the
/// normalised record key ([`norm`]); each holds the last event at or
/// before the second.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    pub second: f64,
    pub player: Option<PlayerAt>,
    /// The run's last enable (`true`) or disable of each reference.
    pub visibility: BTreeMap<FormKey, bool>,
    /// (second, place, source) of each moved reference.
    pub moves: BTreeMap<FormKey, (f64, Spot, String)>,
    /// (second, place, source) of each actor.
    pub actors: BTreeMap<FormKey, (f64, Spot, String)>,
    /// The last stage set of each quest, with its second.
    pub stages: BTreeMap<FormKey, (i32, f64)>,
    /// The last stage set of any quest.
    pub last_stage: Option<(FormKey, i32, f64)>,
    /// Objective changes by quest and objective: the last value of each
    /// kind with its second.
    pub objectives: BTreeMap<(FormKey, i32), ObjectiveValues>,
    /// Lines whose timed seconds cover this second, with their start.
    pub spoken: Vec<(f64, Line)>,
    /// The last line started at or before this second.
    pub last_line: Option<(f64, Line)>,
    /// NoWait visibility changes at or before this second left out
    /// (`timeline_nowait_visibility_v1` off).
    pub nowait_left: usize,
    /// Rules the frame's own readings relied on.
    pub rules: BTreeSet<&'static str>,
    /// The readings the frame was taken with.
    pub readings: Readings,
    /// Actors the run has dead at or before this second, with the second
    /// of the `death` line ([`body_note`]); kept whatever the readings.
    pub dead: BTreeMap<FormKey, f64>,
    /// Who sits in which furniture reference at this second, with the
    /// second of the `sit` line; kept whatever the readings.
    pub sitting: BTreeMap<Sitter, (f64, FormKey)>,
}

fn xy_dir(a: [f64; 3], b: [f64; 3]) -> Option<[f64; 2]> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l = dx.hypot(dy);
    (l > MIN_MOVE).then(|| [dx / l, dy / l])
}

type Point<'a> = (f64, &'a FormKey, &'a Option<FormKey>, [f64; 3]);

fn player_at(points: &[Point<'_>], s: f64) -> Option<PlayerAt> {
    let first = points.first()?;
    if s < first.0 {
        return None;
    }
    // The point the position comes from, and the position.
    let mut at = points.len() - 1;
    let mut pos = points[at].3;
    for (i, w) in points.windows(2).enumerate() {
        let (a, b) = (&w[0], &w[1]);
        if s < b.0 {
            at = i;
            pos = if a.1 != b.1 || b.0 <= a.0 {
                a.3
            } else {
                let k = (s - a.0) / (b.0 - a.0);
                std::array::from_fn(|j| a.3[j] + k * (b.3[j] - a.3[j]))
            };
            break;
        }
    }
    let space = points[at].1;
    // A move of one space: (from, to, start, end).
    let moving = |i: usize| -> Option<[f64; 2]> {
        let (a, b) = (points.get(i)?, points.get(i + 1)?);
        (a.1 == b.1 && a.1 == space && b.0 > a.0)
            .then(|| xy_dir(a.3, b.3))
            .flatten()
    };
    let heading = if s < points.last().map_or(s, |p| p.0) {
        moving(at).map(|d| (d, "the track's current move"))
    } else {
        None
    }
    .or_else(|| {
        (0..at)
            .rev()
            .find_map(moving)
            .map(|d| (d, "the track's last move before this second"))
    })
    .or_else(|| {
        (at..points.len())
            .find_map(moving)
            .map(|d| (d, "the track's next move after this second"))
    });
    Some(PlayerAt {
        space: norm(space),
        cell: points[at].2.as_ref().map(norm),
        pos,
        heading,
    })
}

/// The camera of a frame (presentation, the caller's assumptions): the eye
/// `eye_height` units above the player's track position, looking level
/// along `heading_degrees` (counter-clockwise from +X in the XY plane, this
/// module's convention, not the game's) when given, else along the track's
/// direction of movement. Returns (eye, look point, what the direction
/// came from).
pub fn eye_look(
    frame: &Frame,
    eye_height: f64,
    heading_degrees: Option<f64>,
) -> Result<([f64; 3], [f64; 3], String), String> {
    let p = frame
        .player
        .as_ref()
        .ok_or("TimelineNoPlayer: the second is before the track's first point")?;
    if !eye_height.is_finite() {
        return Err("TimelineEyeHeight: not finite".into());
    }
    let (d, from) = match heading_degrees {
        Some(h) if h.is_finite() => {
            let r = h.to_radians();
            (
                [r.cos(), r.sin()],
                format!("the caller's heading {h} degrees"),
            )
        }
        Some(_) => return Err("TimelineHeading: not finite".into()),
        None => match p.heading {
            Some((d, from)) => (d, from.to_string()),
            None => {
                return Err(
                    "TimelineNoHeading: the track never moves in this space; give a heading".into(),
                )
            }
        },
    };
    let eye = [p.pos[0], p.pos[1], p.pos[2] + eye_height];
    let look = [eye[0] + d[0] * 100.0, eye[1] + d[1] * 100.0, eye[2]];
    Ok((eye, look, from))
}

/// Where a frame's camera looks (presentation, the caller's choice;
/// `--look-at`, `--look-at-actor`). The eye stays where [`eye_look`] puts
/// it.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Aim {
    /// Along the track's movement or the caller's heading ([`eye_look`]).
    #[default]
    Track,
    /// At the caller's point.
    Point([f64; 3]),
    /// At the drawn actor nearest the eye, `eye_height` above its place.
    Nearest,
    /// At the speaker of the line being spoken (else of the last line
    /// started) where it is drawn, else as [`Aim::Nearest`].
    Speaker,
}

/// A drawn actor closer to the eye than this (horizontally, world units)
/// is not aimed at (presentation).
const MIN_AIM: f64 = 1.0;

/// The camera of a frame aimed as `aim` says, among the actors drawn in the
/// frame (`drawn`: key and place, characters and markers): (eye, look
/// point, the overlay's camera line). [`Aim::Track`] gives [`eye_look`]
/// and [`camera_text`] unchanged; the others look from the same eye at a
/// point, `eye_height` above an aimed actor's place, and say what they
/// looked at, or fall back with the reason. Presentation only.
pub fn camera(
    frame: &Frame,
    eye_height: f64,
    heading_degrees: Option<f64>,
    aim: &Aim,
    drawn: &[(FormKey, [f64; 3])],
) -> Result<([f64; 3], [f64; 3], String), String> {
    let track = |prefix: &str| -> Result<([f64; 3], [f64; 3], String), String> {
        let (e, l, from) = eye_look(frame, eye_height, heading_degrees)?;
        Ok((
            e,
            l,
            format!("{prefix}{}", camera_text(e, l, eye_height, &from)),
        ))
    };
    if *aim == Aim::Track {
        return track("");
    }
    let p = frame
        .player
        .as_ref()
        .ok_or("TimelineNoPlayer: the second is before the track's first point")?;
    if !eye_height.is_finite() {
        return Err("TimelineEyeHeight: not finite".into());
    }
    let eye = [p.pos[0], p.pos[1], p.pos[2] + eye_height];
    let aimed = |at: [f64; 3], what: String| {
        let look = [at[0], at[1], at[2] + eye_height];
        let text = format!(
            "eye {:.1},{:.1},{:.1} look {:.1},{:.1},{:.1}: {eye_height} units above the track (the caller's), looking at {what} (presentation)",
            eye[0], eye[1], eye[2], look[0], look[1], look[2]
        );
        (eye, look, text)
    };
    let far = |q: &[f64; 3]| (q[0] - eye[0]).hypot(q[1] - eye[1]) >= MIN_AIM;
    let nearest = |prefix: &str| -> Result<([f64; 3], [f64; 3], String), String> {
        let d2 = |q: &[f64; 3]| (0..3).map(|i| (q[i] - eye[i]).powi(2)).sum::<f64>();
        let best = drawn
            .iter()
            .filter(|(_, q)| far(q))
            .min_by(|a, b| d2(&a.1).total_cmp(&d2(&b.1)));
        match best {
            Some((k, q)) => Ok(aimed(
                *q,
                format!(
                    "{prefix}the nearest drawn actor {}, {eye_height} units above its place",
                    key_text(k)
                ),
            )),
            None => track(&format!("{prefix}no drawn actor to look at; ")),
        }
    };
    match aim {
        Aim::Track => unreachable!(),
        Aim::Point(q) => {
            if !q.iter().all(|v| v.is_finite()) || (0..3).all(|i| q[i] == eye[i]) {
                return Err("TimelineLookAt: the point is not finite or is the eye".into());
            }
            let text = format!(
                "eye {:.1},{:.1},{:.1} look {:.1},{:.1},{:.1}: {eye_height} units above the track (the caller's), looking at the caller's point (presentation)",
                eye[0], eye[1], eye[2], q[0], q[1], q[2]
            );
            Ok((eye, *q, text))
        }
        Aim::Nearest => nearest(""),
        Aim::Speaker => {
            let (line, what) = match (frame.spoken.last(), &frame.last_line) {
                (Some((_, l)), _) => (Some(l), "the line being spoken"),
                (None, Some((_, l))) => (Some(l), "the last line started"),
                (None, None) => (None, "no line"),
            };
            let Some(line) = line else {
                return nearest("no line started; ");
            };
            let Some(k) = &line.speaker else {
                return nearest(&format!("the speaker of {what} not resolved; "));
            };
            match drawn.iter().find(|(d, _)| norm(d) == norm(k)) {
                Some((_, q)) if far(q) => Ok(aimed(
                    *q,
                    format!(
                        "the speaker of {what} {}, {eye_height} units above its place",
                        key_text(k)
                    ),
                )),
                Some(_) => nearest(&format!(
                    "the speaker of {what} {} stands at the eye; ",
                    key_text(k)
                )),
                None => nearest(&format!(
                    "the speaker of {what} {} is not drawn in this frame; ",
                    key_text(k)
                )),
            }
        }
    }
}

/// The overlay's camera line (presentation).
pub fn camera_text(eye: [f64; 3], look: [f64; 3], eye_height: f64, from: &str) -> String {
    format!(
        "eye {:.1},{:.1},{:.1} look {:.1},{:.1},{:.1}: {eye_height} units above the track (the caller's), level, along {from} (presentation)",
        eye[0], eye[1], eye[2], look[0], look[1], look[2]
    )
}

/// The cell to load for the player at a frame: the interior cell it is in,
/// or in a worldspace the exterior cell of the same world whose grid square
/// of [`EXTERIOR_CELL_UNITS`] holds the position (`world_cell_natives_v1`),
/// found from the world's grid through the cell the writer recorded. The
/// second part is the rule relied on, for an exterior cell.
pub fn player_cell(
    store: &Store,
    frame: &Frame,
    b: &Budget,
) -> Result<(FormKey, Option<&'static str>), Error> {
    let fail = |why: &str| Error::new("TimelinePlayerCell", "timeline", 0, "a cell", why);
    let p = frame
        .player
        .as_ref()
        .ok_or_else(|| fail("no track point at or before the second"))?;
    match store.signature(&p.space) {
        Some(s) if s == *b"CELL" => Ok((p.space.clone(), None)),
        Some(s) if s == *b"WRLD" => {
            let hint = p
                .cell
                .as_ref()
                .ok_or_else(|| fail("the writer recorded no cell for the track point"))?;
            let g = [
                (p.pos[0] / EXTERIOR_CELL_UNITS).floor() as i32,
                (p.pos[1] / EXTERIOR_CELL_UNITS).floor() as i32,
            ];
            let grid = crate::grid::world_grid(store, hint, b)?;
            if grid.world.as_ref().map(norm) != Some(p.space.clone()) {
                return Err(fail("the recorded cell is not in the track's worldspace"));
            }
            match grid.at(g) {
                (Some(k), _) => Ok((k.clone(), Some(hypotheses::WORLD_CELL))),
                (None, why) => Err(fail(why)),
            }
        }
        _ => Err(fail("the track's space names no winning CELL or WRLD")),
    }
}

/// The space (interior cell or worldspace) a loaded scene stands in.
pub fn scene_space(store: &Store, scene: &Scene) -> FormKey {
    norm(
        &store
            .world(&scene.cell.key)
            .unwrap_or_else(|| scene.cell.key.clone()),
    )
}

/// What [`apply`] did to a scene.
#[derive(Debug, Clone, Default)]
pub struct Applied {
    /// Drawn references the run disabled, taken out.
    pub hidden: Vec<FormKey>,
    /// Initially disabled references the run enabled, put in.
    pub shown: Vec<FormKey>,
    /// Enabled or disabled references not among the loaded cells' own.
    pub elsewhere: usize,
    /// Instances whose enable parent chain now gives another state.
    pub chain_changed: usize,
    /// References drawn at the place the run moved them to.
    pub moved: Vec<(FormKey, [f64; 3])>,
    /// Moved references left out: moved to another space or to an
    /// unknown place (presentation: not drawn where it is not known).
    pub moved_out: Vec<(FormKey, String)>,
    /// Rules relied on.
    pub rules: BTreeSet<&'static str>,
}

/// Applies a frame's visibility changes and moves to a loaded scene, before
/// its draw list is built: a reference whose last change is a disable is
/// taken out, an initially disabled one whose last change is an enable is
/// loaded and put in (`vm_disable_hides_reference_v1`); every instance whose
/// enable parent chain names a changed reference takes the state the chain
/// now gives (`xesp_script_change_propagates_v1`); a moved reference is
/// placed at its new position with its stored rotation and scale
/// (`timeline_move_stored_rotation_v1`, its position from the run's
/// reading), left out when moved to another space or to an unknown place.
/// The rules are pending: anything to apply needs `RuleLevel::Hypotheses`.
pub fn apply(
    scene: &mut Scene,
    loader: &mut Loader<'_>,
    frame: &Frame,
    level: RuleLevel,
    budget: &Budget,
) -> Result<Applied, Error> {
    let mut out = Applied::default();
    if frame.visibility.is_empty() && frame.moves.is_empty() {
        return Ok(out);
    }
    if level != RuleLevel::Hypotheses {
        return Err(Error::new(
            "TimelineRuleLevel",
            "timeline",
            0,
            "--under-hypotheses",
            format!(
                "the changes rely on the pending rules {}, {} and {}",
                hypotheses::DISABLE_HIDES,
                hypotheses::ENABLE_SCRIPT,
                hypotheses::TIMELINE_MOVE
            ),
        ));
    }
    let space = scene_space(loader.plugins, scene);
    for (k, shown) in &frame.visibility {
        let at = scene
            .instances
            .iter()
            .position(|i| norm(&i.reference.key) == *k);
        match (shown, at) {
            (false, Some(i)) => {
                scene.instances.remove(i);
                out.hidden.push(k.clone());
                out.rules.insert(hypotheses::DISABLE_HIDES);
            }
            (true, Some(_)) => {}
            (true, None) => {
                let Some(d) = scene.disabled.iter().position(|c| norm(&c.key) == *k) else {
                    out.elsewhere += 1;
                    continue;
                };
                // An actor the run enabled is drawn by the frame's markers
                // or cast (`markers`), never as a placed instance: loading
                // it as one failed (`RecordTypeMismatch`).
                if scene.disabled[d].signature != *b"REFR" {
                    continue;
                }
                let child = scene.disabled.remove(d);
                let scope = budget.child();
                let instance = loader.instance(&child.key, &scope)?;
                scope.commit(budget)?;
                scene.assets.extend_from_slice(&instance.assets);
                scene.assets.sort_unstable();
                scene.assets.dedup();
                scene.instances.push(instance);
                out.shown.push(k.clone());
                out.rules.insert(hypotheses::DISABLE_HIDES);
            }
            (false, None) => out.elsewhere += 1,
        }
    }
    let set = |k: &FormKey| frame.visibility.get(&norm(k)).copied();
    for inst in &mut scene.instances {
        let Some(p) = &inst.enable else { continue };
        if !p
            .chain
            .nodes
            .iter()
            .any(|x| frame.visibility.contains_key(&norm(&x.key)))
        {
            continue;
        }
        let (before, after) = (p.enabled, p.with(&set));
        if before != after {
            inst.set_enabled(after);
            out.chain_changed += 1;
            out.rules.insert(hypotheses::ENABLE_SCRIPT);
        }
    }
    let mut gone = Vec::new();
    for (k, (_, at, _)) in &frame.moves {
        let Some(i) = scene
            .instances
            .iter()
            .position(|i| norm(&i.reference.key) == *k)
        else {
            continue;
        };
        match at {
            Spot::At { space: s, pos } if norm(s) == space => {
                let inst = &mut scene.instances[i];
                if let Some(t) = &mut inst.reference.data.transform {
                    t[0] = pos[0] as f32;
                    t[1] = pos[1] as f32;
                    t[2] = pos[2] as f32;
                    out.moved.push((k.clone(), *pos));
                    out.rules.insert(hypotheses::TIMELINE_MOVE);
                }
            }
            Spot::At { space: s, .. } => {
                gone.push(i);
                out.moved_out
                    .push((k.clone(), format!("moved to {}", key_text(s))));
            }
            Spot::Open(why) => {
                gone.push(i);
                out.moved_out.push((k.clone(), why.clone()));
            }
        }
    }
    gone.sort_unstable();
    for i in gone.into_iter().rev() {
        scene.instances.remove(i);
    }
    Ok(out)
}

/// An actor's placeholder marker (presentation): where it stands.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub reference: FormKey,
    pub pos: [f64; 3],
    pub source: String,
    /// How the actor is drawn there: standing unless a reading of the
    /// run's deaths or sits says otherwise ([`Readings::dead`],
    /// [`Readings::sit_marker`]).
    pub pose: Pose,
}

/// How a marked actor is drawn ([`Marker::pose`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Pose {
    /// As before: standing at its place.
    #[default]
    Standing,
    /// Dead from `since`, drawn lying (`timeline_dead_actor_lying_placeholder_v1`).
    Lying { since: f64 },
    /// Sitting from `since` in the furniture reference `furniture` (base
    /// `base`), placed at its marker `seat` facing `heading` (radians about
    /// the vertical axis, the placement rotation's convention)
    /// (`timeline_sit_marker_nearest_v1`).
    Seated {
        since: f64,
        furniture: FormKey,
        base: FormKey,
        seat: usize,
        heading: f32,
    },
}

/// The rotation a dead actor lying is drawn with
/// (`timeline_dead_actor_lying_placeholder_v1`, a placeholder): its
/// placement's rotation about the vertical axis kept, then a quarter turn
/// about the world's X axis, so that its up axis lies along the ground.
pub fn lying_rotation(rotation: [f32; 3]) -> [f32; 3] {
    [std::f32::consts::FRAC_PI_2, 0., rotation[2]]
}

/// How a frame decides whether an actor is present ([`presence`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Presence {
    /// The actor's own last Enable (`true`) or Disable in the run
    /// (`vm_disable_hides_reference_v1`).
    Run(bool),
    /// Its enable parent chain under the enable parent rules, with the
    /// run's changes of the chain's records.
    Chain { present: bool },
    /// No enable parent: its own initially-disabled flag (clear: present).
    Flag(bool),
    /// The chain carries what no rule in use decides: the `found` texts
    /// of `crate::enable` left open.
    Undecided(Vec<&'static str>),
    /// No winning record.
    NoRecord,
}
impl Presence {
    /// Present, absent, or `None` when not decided.
    pub fn present(&self) -> Option<bool> {
        match self {
            Self::Run(v) | Self::Flag(v) => Some(*v),
            Self::Chain { present } => Some(*present),
            Self::Undecided(_) | Self::NoRecord => None,
        }
    }
}

/// [`presence_with_rules`] without the rules.
pub fn presence(store: &Store, frame: &Frame, k: &FormKey, b: &Budget) -> Result<Presence, Error> {
    Ok(presence_with_rules(store, frame, k, b)?.0)
}

/// Whether actor `k` is present at the frame: its own last change in the
/// run (an enable present, a disable absent: `vm_disable_hides_reference_v1`),
/// else its enable parent chain under the enable parent rules with the
/// run's changes (`xesp_script_change_propagates_v1`), and with
/// [`Readings::child_flag_agrees`] a chain left open only by records below
/// its root carrying their own flag 0x800, each agreeing with the chain's
/// start state, read as any other (`xesp_child_flag_agrees_chain_v1`),
/// else without a chain its
/// initially-disabled flag. Returns the rules relied on (for a chain,
/// every covered part, decided or not).
pub fn presence_with_rules(
    store: &Store,
    frame: &Frame,
    k: &FormKey,
    b: &Budget,
) -> Result<(Presence, BTreeSet<&'static str>), Error> {
    let mut rules = BTreeSet::new();
    let set = |k: &FormKey| frame.visibility.get(&norm(k)).copied();
    if let Some(v) = set(k) {
        rules.insert(hypotheses::DISABLE_HIDES);
        return Ok((Presence::Run(v), rules));
    }
    let p = match fo4_plugin::enable::chain(store, k, b)? {
        // A chain with a parent: the enable parent rules decide.
        Some(chain) if chain.nodes.len() > 1 => {
            let found = enable::findings(&chain);
            for f in &found {
                if let Some(r) = enable::rule(f) {
                    rules.insert(r);
                }
            }
            let open: Vec<&'static str> = found
                .iter()
                .copied()
                .filter(|f| enable::rule(f).is_none())
                .collect();
            let p = enable::Presence::of(chain);
            let agreeing = open == [enable::FOUND_OWN_FLAG]
                && frame.readings.child_flag_agrees
                && enable::child_flags_agree(&p.chain);
            let after = if agreeing {
                rules.insert(hypotheses::ENABLE_CHILD_FLAG_AGREES);
                p.with_agreeing_child_flags(&set)
            } else {
                p.with(&set)
            };
            match after {
                Some(present) => {
                    if after != p.enabled {
                        rules.insert(hypotheses::ENABLE_SCRIPT);
                    }
                    Presence::Chain { present }
                }
                None => Presence::Undecided(open),
            }
        }
        // No parent: the record's initially-disabled flag.
        _ => match store.overrides().winner(k) {
            Some(w) => Presence::Flag(!w.disabled),
            None => Presence::NoRecord,
        },
    };
    Ok((p, rules))
}

/// The actors of a frame standing in `space`, and why each other one is
/// not marked. An actor is present as [`presence_with_rules`] decides.
/// Returns the markers, one line per actor left out, and the rules relied
/// on.
pub fn markers(
    store: &Store,
    frame: &Frame,
    space: &FormKey,
    b: &Budget,
) -> Result<Markers, Error> {
    let mut out = Vec::new();
    let mut left = Vec::new();
    let mut rules = BTreeSet::new();
    for (k, (_, at, source)) in &frame.actors {
        let (s, pos) = match at {
            Spot::At { space, pos } => (norm(space), *pos),
            Spot::Open(why) => {
                left.push(format!("{}: position unknown ({why})", key_text(k)));
                continue;
            }
        };
        if s != *space {
            continue;
        }
        let (p, r) = presence_with_rules(store, frame, k, b)?;
        rules.extend(r);
        match p.present() {
            Some(true) => {
                let mut m = Marker {
                    reference: k.clone(),
                    pos,
                    source: source.clone(),
                    pose: Pose::Standing,
                };
                if let Some(t) = frame.dead.get(k) {
                    match frame.readings.dead {
                        Dead::Standing => {}
                        Dead::LeftOut => {
                            rules.insert(hypotheses::TIMELINE_DEAD_LEFT_OUT);
                            left.push(format!(
                                "{}: dead from {t} s ({})",
                                key_text(k),
                                hypotheses::TIMELINE_DEAD_LEFT_OUT
                            ));
                            continue;
                        }
                        Dead::Lying => {
                            rules.insert(hypotheses::TIMELINE_DEAD_LYING);
                            m.pose = Pose::Lying { since: *t };
                            m.source = format!(
                                "{source}; dead from {t} s, drawn lying at its last place (placeholder, {})",
                                hypotheses::TIMELINE_DEAD_LYING
                            );
                        }
                    }
                } else if let (true, Some((t, f))) = (
                    frame.readings.sit_marker,
                    frame.sitting.get(&Sitter::Actor(k.clone())),
                ) {
                    match crate::furniture::seat_near(store, f, pos, b)? {
                        Ok(seat) => {
                            rules.insert(hypotheses::TIMELINE_SIT_MARKER);
                            rules.insert(hypotheses::FURNITURE_MARKER_SNAM);
                            m.pos = seat.pos;
                            m.source = format!(
                                "{source}; sitting in {} from {t} s at its marker {} of {} ({}, {})",
                                key_text(f),
                                seat.index,
                                seat.of,
                                hypotheses::TIMELINE_SIT_MARKER,
                                hypotheses::FURNITURE_MARKER_SNAM
                            );
                            m.pose = Pose::Seated {
                                since: *t,
                                furniture: f.clone(),
                                base: seat.base,
                                seat: seat.index,
                                heading: seat.heading,
                            };
                        }
                        Err(why) => {
                            m.source = format!(
                                "{source}; sitting in {} from {t} s, standing: {why}",
                                key_text(f)
                            );
                        }
                    }
                }
                out.push(m);
            }
            Some(false) => left.push(format!("{}: disabled", key_text(k))),
            None => left.push(format!(
                "{}: presence undetermined by its enable chain",
                key_text(k)
            )),
        }
    }
    Ok((out, left, rules))
}

/// The overlay text of a frame (presentation): the second, the player's
/// place, the camera's source, the stages, the objectives displayed and
/// not completed, the line being spoken and the last line started.
pub fn overlay(t: &Timeline, f: &Frame, camera: &str) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "second\t{}", f.second);
    match &f.player {
        Some(p) => {
            let _ = writeln!(
                s,
                "player\t{}\t{:.1} {:.1} {:.1}",
                t.label(&p.space),
                p.pos[0],
                p.pos[1],
                p.pos[2]
            );
        }
        None => s.push_str("player\tbefore the track\n"),
    }
    let _ = writeln!(s, "camera\t{camera}");
    if let Some((q, n, at)) = &f.last_stage {
        let _ = writeln!(s, "last stage\t{} stage {n} at {at} s", t.label(q));
    }
    // The quests with an objective change so far, and the quest of the
    // last stage; the others only counted (presentation).
    let listed: BTreeSet<&FormKey> = f
        .objectives
        .keys()
        .map(|(q, _)| q)
        .chain(f.last_stage.as_ref().map(|(q, ..)| q))
        .collect();
    for (q, (n, at)) in f.stages.iter().filter(|(q, _)| listed.contains(q)) {
        let _ = writeln!(s, "stage\t{} {n} (set at {at} s)", t.label(q));
    }
    let others = f.stages.keys().filter(|q| !listed.contains(q)).count();
    if others > 0 {
        let _ = writeln!(
            s,
            "stages\t{others} other quests have a stage set (not listed)"
        );
    }
    for ((q, o), kinds) in &f.objectives {
        let shown = kinds
            .get(&ObjectiveKind::Displayed)
            .is_some_and(|v| v.0 == Some(true));
        let done = kinds
            .get(&ObjectiveKind::Completed)
            .is_some_and(|v| v.0 == Some(true));
        if shown && !done {
            let _ = writeln!(s, "objective\t{} {o}", t.label(q));
        }
    }
    let line = |s: &mut String, what: &str, at: f64, l: &Line| {
        let _ = writeln!(
            s,
            "{what}\t{} s, {} s long\t{}\t{}",
            at,
            l.seconds.map_or("?".into(), |x| format!("{x:.2}")),
            l.speaker
                .as_ref()
                .map_or("speaker not resolved".into(), |k| t.label(k)),
            l.text.as_deref().unwrap_or("(no text)")
        );
    };
    if f.readings.sit_marker {
        if let Some((at, furniture)) = f.sitting.get(&Sitter::Player) {
            let _ = writeln!(
                s,
                "sitting\tplayer in {} from {at} s (the run's sit): the camera stays on the track, the run's own model of the player's place",
                t.label(furniture)
            );
        }
    }
    for (at, l) in &f.spoken {
        line(&mut s, "spoken", *at, l);
    }
    if f.spoken.is_empty() {
        if let Some((at, l)) = &f.last_line {
            line(&mut s, "last line", *at, l);
        }
    }
    s
}

/// The overlay's lines on what a frame applied ([`apply`]) and drew (the
/// markers with their tags, the actors left out), and the rules relied on.
pub fn report(
    frame: &Frame,
    applied: &Applied,
    markers: &[Marker],
    left: &[String],
    rules: &std::collections::BTreeSet<&'static str>,
    first: u32,
    tags: &[u32],
) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "applied\t{} hidden, {} shown, {} changes on references of other cells, {} enable children changed, {} moved, {} moved out; {} NoWait changes left out",
        applied.hidden.len(),
        applied.shown.len(),
        applied.elsewhere,
        applied.chain_changed,
        applied.moved.len(),
        applied.moved_out.len(),
        frame.nowait_left
    );
    for (m, tag) in markers.iter().zip(tags) {
        let _ = writeln!(
            s,
            "marker\ttag {tag}\t{}\t{:.1} {:.1} {:.1}\t{}",
            key_text(&m.reference),
            m.pos[0],
            m.pos[1],
            m.pos[2],
            m.source
        );
    }
    if tags.is_empty() {
        let _ = writeln!(s, "marker\tnone (first tag would be {first})");
    }
    for l in left {
        let _ = writeln!(s, "actor left out\t{l}");
    }
    for (k, why) in &applied.moved_out {
        let _ = writeln!(s, "moved out\t{}\t{why}", key_text(k));
    }
    let r: Vec<&str> = rules.iter().copied().collect();
    let _ = writeln!(s, "rules\t{}", r.join(", "));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(id: u32) -> FormKey {
        FormKey {
            plugin: "base.esm".into(),
            object_id: id,
        }
    }

    fn sample() -> Timeline {
        let w = k(0x10);
        let mut t = Timeline::default();
        t.notes.push("a note\twith a tab".into());
        t.labels.insert(k(0x20), "SomeQuest".into());
        let ev = |t: f64, what: What| Event { t, what };
        t.events = vec![
            ev(
                0.,
                What::Player {
                    space: w.clone(),
                    cell: Some(k(0x11)),
                    pos: [0., 0., 10.],
                    facing: None,
                },
            ),
            ev(
                0.,
                What::Actor {
                    reference: k(0x30),
                    at: Spot::At {
                        space: w.clone(),
                        pos: [100., 0., 0.],
                    },
                    source: "world_actor_placed_position_v1".into(),
                },
            ),
            ev(
                1.5,
                What::Visibility {
                    reference: k(0x40),
                    shown: false,
                    nowait: false,
                },
            ),
            ev(
                1.75,
                What::Visibility {
                    reference: k(0x42),
                    shown: false,
                    nowait: true,
                },
            ),
            ev(
                2.,
                What::Player {
                    space: w.clone(),
                    cell: Some(k(0x11)),
                    pos: [200., 0., 10.],
                    facing: Some(90.),
                },
            ),
            ev(
                2.25,
                What::Move {
                    reference: k(0x41),
                    at: Spot::At {
                        space: w.clone(),
                        pos: [5., 6., 7.],
                    },
                    source: "world_moveto_target_v1".into(),
                },
            ),
            ev(
                2.25,
                What::Actor {
                    reference: k(0x31),
                    at: Spot::Open("a native of the run may have moved it".into()),
                    source: "world_moveto_target_v1".into(),
                },
            ),
            ev(
                3.,
                What::Stage {
                    quest: k(0x20),
                    stage: 10,
                },
            ),
            ev(
                3.,
                What::Objective {
                    quest: k(0x20),
                    objective: 5,
                    kind: ObjectiveKind::Displayed,
                    value: Some(true),
                },
            ),
            ev(
                3.,
                What::Quest {
                    quest: k(0x20),
                    change: "Started".into(),
                },
            ),
            ev(
                4.,
                What::Scene {
                    scene: k(0x50),
                    change: "PhaseEntered(0)".into(),
                },
            ),
            ev(
                4.,
                What::Line(Line {
                    scene: k(0x50),
                    phase: 0,
                    action: 2,
                    speaker: Some(k(0x30)),
                    seconds: Some(2.5),
                    topic: Some(k(0x51)),
                    how: "line: seconds from its audio".into(),
                    text: Some("Hello,\nthere \\ friend.".into()),
                }),
            ),
            ev(
                5.,
                What::Line(Line {
                    scene: k(0x50),
                    phase: 1,
                    action: 3,
                    speaker: None,
                    seconds: None,
                    topic: None,
                    how: "line: undetermined".into(),
                    text: None,
                }),
            ),
            ev(5., What::Note("enable of an alias".into())),
            ev(
                10.,
                What::Player {
                    space: k(0x12),
                    cell: Some(k(0x12)),
                    pos: [1., 2., 3.],
                    facing: None,
                },
            ),
        ];
        t
    }

    #[test]
    fn a_timeline_round_trips_through_its_text() {
        let t = sample();
        let text = t.write();
        assert!(text.starts_with("osp-run-timeline\t1\n"));
        let back = Timeline::parse(&text).unwrap();
        assert_eq!(back, t);
        assert_eq!(back.write(), text);
        // Seconds and positions keep every bit.
        let mut odd = Timeline::default();
        odd.events.push(Event {
            t: 0.1 + 0.2,
            what: What::Player {
                space: k(1),
                cell: None,
                pos: [-80302.9, 1.0 / 3.0, 7854.98],
                facing: None,
            },
        });
        assert_eq!(Timeline::parse(&odd.write()).unwrap(), odd);
    }

    #[test]
    fn a_timeline_refuses_other_formats_and_bad_lines() {
        assert!(Timeline::parse("").is_err());
        assert!(Timeline::parse("osp-run-timeline\t2\n").is_err());
        let ok = "osp-run-timeline\t1\n";
        assert!(Timeline::parse(ok).unwrap().events.is_empty());
        for bad in [
            "stage\t1\tbase.esm:00000020\n",
            "stage\tx\tbase.esm:00000020\t1\n",
            "stage\t1\tbase.esm:20\t1\n",
            "visibility\t1\tbase.esm:00000020\tmaybe\tplain\n",
            "visibility\t1\tbase.esm:00000020\tenabled\n",
            "jump\t1\n",
            "stage\t2\tbase.esm:00000020\t1\nstage\t1\tbase.esm:00000020\t2\n",
            "event\t1\tbad \\q escape\n",
        ] {
            assert!(Timeline::parse(&format!("{ok}{bad}")).is_err(), "{bad}");
        }
    }

    /// The death, sit and get-up event lines are read back (the writer's
    /// grammar, `spec/timeline.md`); a frame keeps who is dead from when and
    /// who sits where, a get-up ending only its own sit.
    #[test]
    fn a_frame_keeps_the_runs_deaths_and_sits() {
        assert_eq!(
            body_note("death base.esm:00000030"),
            Some(BodyNote::Death(Some(k(0x30))))
        );
        assert_eq!(body_note("death -"), Some(BodyNote::Death(None)));
        assert_eq!(
            body_note("sit player Base.esm:00000050"),
            Some(BodyNote::Sit(Sitter::Player, k(0x50)))
        );
        assert_eq!(body_note("sit player"), None);
        assert_eq!(body_note("an enable of something"), None);
        let ev = |t: f64, text: &str| Event {
            t,
            what: What::Note(text.into()),
        };
        let t = Timeline {
            events: vec![
                ev(1., "death base.esm:00000030"),
                ev(2., "death base.esm:00000030"),
                ev(2., "sit base.esm:00000031 base.esm:00000050"),
                ev(3., "sit player base.esm:00000051"),
                ev(4., "get-up base.esm:00000031 base.esm:00000052"),
                ev(5., "get-up base.esm:00000031 base.esm:00000050"),
            ],
            ..Timeline::default()
        };
        let f = t.at(0.5);
        assert!(f.dead.is_empty() && f.sitting.is_empty());
        let f = t.at(4.5);
        assert_eq!(f.dead.get(&k(0x30)), Some(&1.));
        assert_eq!(f.sitting.get(&Sitter::Actor(k(0x31))), Some(&(2., k(0x50))));
        assert_eq!(f.sitting.get(&Sitter::Player), Some(&(3., k(0x51))));
        let f = t.at(5.);
        assert!(!f.sitting.contains_key(&Sitter::Actor(k(0x31))));
        // Readings off: the overlay is as before; the player's sit is said
        // only with the sit reading.
        assert!(!overlay(&t, &f, "c").contains("sitting"));
        let on = t.at_with(
            5.,
            Readings {
                sit_marker: true,
                ..Readings::default()
            },
        );
        assert!(overlay(&t, &on, "c").contains("sitting\tplayer"));
        // The lying placeholder keeps the rotation about the vertical.
        assert_eq!(
            lying_rotation([0.3, 0.2, 1.5]),
            [std::f32::consts::FRAC_PI_2, 0., 1.5]
        );
    }

    #[test]
    fn a_frame_holds_the_last_change_of_each_thing_at_its_second() {
        let t = sample();
        let f = t.at(1.0);
        assert!(f.visibility.is_empty());
        assert_eq!(f.actors.len(), 1);
        let f = t.at(2.25);
        assert_eq!(f.visibility.get(&k(0x40)), Some(&false));
        assert_eq!(
            f.moves[&k(0x41)].1,
            Spot::At {
                space: k(0x10),
                pos: [5., 6., 7.]
            }
        );
        assert!(matches!(f.actors[&k(0x31)].1, Spot::Open(_)));
        // A NoWait change counts only under its reading.
        assert_eq!(f.nowait_left, 1);
        assert!(!f.visibility.contains_key(&k(0x42)));
        let n = t.at_with(
            2.25,
            Readings {
                nowait: true,
                ..Readings::default()
            },
        );
        assert_eq!(n.visibility.get(&k(0x42)), Some(&false));
        assert_eq!(n.nowait_left, 0);
        assert!(n.rules.contains(hypotheses::TIMELINE_NOWAIT));
        assert!(f.last_stage.is_none());
        let f = t.at(4.5);
        assert_eq!(f.last_stage, Some((k(0x20), 10, 3.)));
        assert_eq!(f.spoken.len(), 1);
        assert!(overlay(&t, &f, "test").contains("SomeQuest base.esm:00000020 5"));
        // The line ends at 6.5 s; the next one has no seconds.
        let f = t.at(6.5);
        assert!(f.spoken.is_empty());
        assert_eq!(f.last_line.as_ref().map(|l| l.1.action), Some(3));
        // Keys are compared with the plugin in lower case.
        assert_eq!(
            f.visibility.get(&norm(&FormKey {
                plugin: "Base.ESM".into(),
                object_id: 0x40
            })),
            Some(&false)
        );
    }

    #[test]
    fn the_camera_follows_the_track_at_the_eye_height_the_caller_gives() {
        let t = sample();
        // Before the first point: no player.
        let mut early = t.clone();
        early
            .events
            .retain(|e| matches!(e.what, What::Player { .. }));
        early.events[0].t = 1.0;
        assert!(early.at(0.5).player.is_none());
        assert!(eye_look(&early.at(0.5), 120., None).is_err());
        // Half way along the first move, looking along it.
        let (eye, look, from) = eye_look(&t.at(1.0), 120., None).unwrap();
        assert_eq!(eye, [100., 0., 130.]);
        assert_eq!(look, [200., 0., 130.]);
        assert_eq!(from, "the track's current move");
        // Standing after the move: the last move's direction.
        let (eye, _, from) = eye_look(&t.at(3.0), 100., None).unwrap();
        assert_eq!(eye, [200., 0., 110.]);
        assert_eq!(from, "the track's last move before this second");
        // In the second space, which has no move: the caller's heading.
        let f = t.at(12.0);
        assert_eq!(f.player.as_ref().unwrap().space, k(0x12));
        assert!(eye_look(&f, 100., None).is_err());
        let (eye, look, _) = eye_look(&f, 100., Some(90.)).unwrap();
        assert_eq!(eye, [1., 2., 103.]);
        assert!((look[0] - 1.).abs() < 1e-9 && (look[1] - 102.).abs() < 1e-9);
        // Until the point of the other space, the player stays in the first.
        let f = t.at(9.99);
        assert_eq!(f.player.unwrap().pos, [200., 0., 10.]);
    }

    #[test]
    fn the_camera_aims_at_a_point_the_nearest_actor_or_the_speaker() {
        let t = sample();
        let f = t.at(4.5);
        // The track's aim is eye_look and camera_text unchanged.
        let (e, l, from) = eye_look(&f, 120., None).unwrap();
        let (e2, l2, text) = camera(&f, 120., None, &Aim::Track, &[]).unwrap();
        assert_eq!((e, l), (e2, l2));
        assert_eq!(text, camera_text(e, l, 120., &from));
        // A point.
        let (_, l, text) = camera(&f, 120., None, &Aim::Point([1., 2., 3.]), &[]).unwrap();
        assert_eq!(l, [1., 2., 3.]);
        assert!(text.contains("the caller's point"), "{text}");
        assert!(camera(&f, 120., None, &Aim::Point(e), &[]).is_err());
        // The nearest drawn actor, eye height above its place; one at the
        // eye is passed over.
        let drawn = vec![
            (k(0x30), [100., 0., 0.]),
            (k(0x31), [200.5, 0., 0.]),
            (k(0x32), [600., 0., 0.]),
        ];
        let (eye, l, text) = camera(&f, 120., None, &Aim::Nearest, &drawn).unwrap();
        assert_eq!(eye, [200., 0., 130.]);
        assert_eq!(l, [100., 0., 120.]);
        assert!(text.contains("base.esm:00000030"), "{text}");
        // No actor: back to the track, saying so.
        let (_, _, text) = camera(&f, 120., None, &Aim::Nearest, &[]).unwrap();
        assert!(text.starts_with("no drawn actor to look at; eye"), "{text}");
        // The speaker of the line being spoken (0x30 at 4.5 s).
        let far = vec![(k(0x30), [600., 0., 0.]), (k(0x31), [100., 0., 0.])];
        let (_, l, text) = camera(&f, 120., None, &Aim::Speaker, &far).unwrap();
        assert_eq!(l, [600., 0., 120.]);
        assert!(text.contains("the line being spoken"), "{text}");
        // Not drawn: the nearest, saying why.
        let (_, l, text) = camera(&f, 120., None, &Aim::Speaker, &far[1..]).unwrap();
        assert_eq!(l, [100., 0., 120.]);
        assert!(text.contains("is not drawn in this frame"), "{text}");
        // At 6.5 s the last line has no resolved speaker.
        let (_, _, text) = camera(&t.at(6.5), 120., None, &Aim::Speaker, &far).unwrap();
        assert!(text.contains("not resolved"), "{text}");
    }
}
