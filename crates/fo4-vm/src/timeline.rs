//! The run's timeline writer (`quest_run new-game --timeline FILE`,
//! `spec/timeline.md`): what a clocked run did, stamped with the seconds of
//! its real-time clock, in the neutral format of `fo4_scene::timeline`.
//!
//! The [`Recorder`] only reads the run: [`Recorder::observe`] takes `&Vm`
//! after each step of the caller's clock loop and keeps what the logs grew
//! by since the last look (effects, quest and scene changes, moves the
//! world model's natives made), stamped with the run's present second;
//! [`Recorder::finish`] adds what the world model kept with its own seconds
//! (the caller's track, the stored and assumed actor positions) and reads
//! labels and line texts from the store. Nothing it does changes the run,
//! its counts or its rule list: a run with a timeline writes every other
//! output byte for byte as one without.
use crate::{
    bind,
    data::key_record,
    quest::{self, Change as QuestChange},
    scene::{self, Change as SceneChange},
    world::{Clock, ObjId, Val, World},
    Vm,
};
use fo4_core::Budget;
use fo4_plugin::{typed::Store, FormKey};
use fo4_scene::timeline::{self as tl, Event, Line, ObjectiveKind, Spot, Timeline, What};
use std::collections::{BTreeMap, BTreeSet};

/// What the writer states in the file's header.
pub const NOTES: [&str; 6] = [
    "written by quest_run new-game --timeline: seconds are the run's real-time clock; every place, change and line is the run's, under the pending rules and caller assumptions of its command line",
    "player: the caller's track points (the run's own model of the player); cell: the interior cell, or the exterior cell whose grid square holds the point (world_cell_natives_v1's reading)",
    "actor: stored positions are read from the second 0; assumed and package moves at their own second (a move is a package destination's when the writer saw it scheduled); native moves at the second of the native",
    "visibility: the run's Enable and Disable natives, and their NoWait forms marked nowait; on an object that is no placed record (an alias, an undetermined value) only an event line",
    "line: a dialogue or player-dialogue action as the scene clock started it; the speaker is the reference the alias held in the run where the writer could read it without changing the run; the text only where the topic holds one line and its strings table settles each response",
    "stage, objective, quest, scene: the run's quest and scene logs as they grew",
];

/// A move the world model's natives made, waiting for its kind (actor or
/// reference) until [`Recorder::finish`] reads the store.
#[derive(Debug, Clone, PartialEq)]
struct NativeMove {
    t: f64,
    key: FormKey,
    to: Option<(FormKey, [f64; 3])>,
}

/// A line as the run started it, waiting for its text.
#[derive(Debug, Clone)]
struct Started {
    t: f64,
    line: Line,
    /// The topic's lines are counted and read at the end.
    kind: u16,
}

/// Reads a run as it goes ([`Recorder::observe`]) and writes its timeline
/// at the end ([`Recorder::finish`]).
#[derive(Debug, Default)]
pub struct Recorder {
    effects: usize,
    quests: usize,
    scenes: usize,
    native_at: BTreeMap<ObjId, (f64, FormKey, [f64; 3])>,
    moved_at: BTreeMap<ObjId, f64>,
    events: Vec<Event>,
    moves: Vec<NativeMove>,
    lines: Vec<Started>,
    /// The package destinations' actor moves seen scheduled (the world
    /// model drops them when their scene ends, applied or not).
    package_moves: Vec<crate::track::ActorMove>,
    /// Times [`Recorder::observe`] ran.
    pub observations: usize,
    /// Write the actors the run killed as `event` lines `death REF`
    /// (`quest_run --timeline-deaths`; `spec/timeline.md`, Deaths and
    /// furniture); off by default: nothing more is written.
    pub deaths: bool,
    /// Write who the run has sitting in which furniture reference as
    /// `event` lines `sit WHO FURNITURE` and `get-up WHO FURNITURE`
    /// (`quest_run --timeline-sits`); off by default.
    pub sits: bool,
    dead_seen: BTreeSet<ObjId>,
    sitting_seen: BTreeMap<ObjId, ObjId>,
}

/// The event text of a death (`--timeline-deaths`).
pub const DEATH: &str = "death";
/// The event texts of a sit and a get-up (`--timeline-sits`).
pub const SIT: &str = "sit";
pub const GET_UP: &str = "get-up";

fn object_key(w: &World, o: ObjId) -> Option<FormKey> {
    w.objects
        .get(o)?
        .key
        .as_deref()
        .and_then(key_record)
        .map(|k| tl::norm(&k))
}

/// The reference an alias of a quest holds in the run, read without
/// changing the run: a fill the run set, else the alias's forced or
/// location reference, else its unique actor's one placed actor. `Err`
/// says why not.
fn alias_reference(w: &World, quest: &FormKey, alias: i32) -> Result<FormKey, String> {
    if alias < 0 {
        return Err("the action names no alias".into());
    }
    let qk = bind::record_key(quest);
    if let Some(v) = w.quests.states.get(&qk).and_then(|s| s.fills.get(&alias)) {
        return match v {
            Val::Object(o) if w.player == Some(*o) => Err("the player".into()),
            Val::Object(o) => object_key(w, *o).ok_or_else(|| "a fill that is no record".into()),
            _ => Err("a fill the run left undetermined".into()),
        };
    }
    let Some(Some(info)) = w.quests.infos.get(&qk) else {
        return Err("the quest's record was not read by the run".into());
    };
    let id = u32::try_from(alias).map_err(|_| "an alias index out of range".to_string())?;
    if let Ok(Some(k)) = info.fill_of(id, w.quests.location_fills) {
        return Ok(tl::norm(&k));
    }
    match (info.unique_actors.get(&id), &w.quests.unique) {
        (Some(base), Some(u)) => match u.fill(base) {
            Ok(quest::Fill::Record(k)) => Ok(tl::norm(&k)),
            Ok(quest::Fill::Player) => Err("the player".into()),
            Err(e) => Err(e.into()),
        },
        _ => Err("the alias is filled by the world".into()),
    }
}

impl Recorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keeps what the run's logs grew by since the last look, stamped with
    /// the run's present second. Reads only.
    pub fn observe(&mut self, vm: &Vm<'_>) {
        self.observations += 1;
        let w = &vm.world;
        let now = w.now[Clock::Real as usize];
        if self.deaths {
            for o in &w.fights.dead {
                if !self.dead_seen.insert(*o) {
                    continue;
                }
                let who = object_key(w, *o).map(|k| tl::key_text(&k));
                self.events.push(Event {
                    t: now,
                    what: What::Note(format!("{DEATH} {}", who.as_deref().unwrap_or("-"))),
                });
            }
        }
        if self.sits {
            let name = |o: ObjId| -> String {
                if w.player == Some(o) {
                    return "player".into();
                }
                object_key(w, o)
                    .map(|k| tl::key_text(&k))
                    .unwrap_or_else(|| "-".into())
            };
            let gone: Vec<(ObjId, ObjId)> = self
                .sitting_seen
                .iter()
                .filter(|(s, f)| w.sits.sitting.get(s) != Some(f))
                .map(|(s, f)| (*s, *f))
                .collect();
            for (s, f) in gone {
                self.sitting_seen.remove(&s);
                self.events.push(Event {
                    t: now,
                    what: What::Note(format!("{GET_UP} {} {}", name(s), name(f))),
                });
            }
            for (s, f) in &w.sits.sitting {
                if self.sitting_seen.get(s) == Some(f) {
                    continue;
                }
                self.sitting_seen.insert(*s, *f);
                self.events.push(Event {
                    t: now,
                    what: What::Note(format!("{SIT} {} {}", name(*s), name(*f))),
                });
            }
        }
        for e in vm.effects.iter().skip(self.effects) {
            let shown = match e.native.as_str() {
                "objectreference.enable" | "objectreference.enablenowait" => true,
                "objectreference.disable" | "objectreference.disablenowait" => false,
                _ => continue,
            };
            let nowait = e.native.ends_with("nowait");
            match e.receiver_object.and_then(|o| object_key(w, o)) {
                Some(reference) => self.events.push(Event {
                    t: now,
                    what: What::Visibility {
                        reference,
                        shown,
                        nowait,
                    },
                }),
                None => self.events.push(Event {
                    t: now,
                    what: What::Note(format!(
                        "{} on {}: not a placed record",
                        e.native,
                        e.receiver.as_deref().unwrap_or("nothing")
                    )),
                }),
            }
        }
        self.effects = vm.effects.len();
        for (o, ch) in w.quests.log.iter().skip(self.quests) {
            let Some(quest) = object_key(w, *o) else {
                continue;
            };
            let what = match ch {
                QuestChange::StageSet(n) => What::Stage { quest, stage: *n },
                QuestChange::Objective(kind, n, v) => What::Objective {
                    quest,
                    objective: *n,
                    kind: match *kind {
                        quest::DISPLAYED => ObjectiveKind::Displayed,
                        quest::COMPLETED => ObjectiveKind::Completed,
                        _ => ObjectiveKind::Failed,
                    },
                    value: *v,
                },
                other => What::Quest {
                    quest,
                    change: format!("{other:?}"),
                },
            };
            self.events.push(Event { t: now, what });
        }
        self.quests = w.quests.log.len();
        for (o, ch) in w.scenes.log.iter().skip(self.scenes) {
            let Some(sk) = object_key(w, *o) else {
                continue;
            };
            if let SceneChange::ActionStarted(p, j, kind, how) = ch {
                if matches!(*kind, scene::LINE | scene::PLAYER_DIALOGUE) {
                    self.lines.push(Started {
                        t: now,
                        line: self.line(w, *o, &sk, *p, *j, *kind, how),
                        kind: *kind,
                    });
                    continue;
                }
            }
            self.events.push(Event {
                t: now,
                what: What::Scene {
                    scene: sk,
                    change: format!("{ch:?}"),
                },
            });
        }
        self.scenes = w.scenes.log.len();
        for (_, m) in &w.place.package_moves {
            if !self.package_moves.contains(m) {
                self.package_moves.push(m.clone());
            }
        }
        // Moves of the world model's natives: a new or later entry.
        for (o, at) in &w.place.native_at {
            if self.native_at.get(o) == Some(at) {
                continue;
            }
            self.native_at.insert(*o, at.clone());
            if let Some(key) = object_key(w, *o) {
                self.moves.push(NativeMove {
                    t: at.0,
                    key,
                    to: Some((tl::norm(&at.1), at.2)),
                });
            }
        }
        for (o, t) in &w.place.moved_at {
            if self.moved_at.get(o) == Some(t) {
                continue;
            }
            self.moved_at.insert(*o, *t);
            let placed = w.place.native_at.get(o).is_some_and(|x| x.0 == *t);
            if let (false, Some(key)) = (placed, object_key(w, *o)) {
                self.moves.push(NativeMove {
                    t: *t,
                    key,
                    to: None,
                });
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn line(
        &self,
        w: &World,
        o: ObjId,
        sk: &FormKey,
        phase: usize,
        action: usize,
        kind: u16,
        how: &str,
    ) -> Line {
        let info = w.scenes.infos.get(&bind::record_key(sk)).cloned().flatten();
        let a = info.as_ref().and_then(|i| i.actions.get(action));
        let seconds = w
            .scenes
            .actions
            .get(&(o, action))
            .and_then(|r| r.due.map(|d| d - r.start));
        let (speaker, said) = match (kind, info.as_ref().and_then(|i| i.quest.clone()), a) {
            (scene::LINE, Some(q), Some(a)) => match alias_reference(w, &q, a.alias) {
                Ok(k) => (Some(k), String::new()),
                Err(why) => (None, format!("; speaker: {why}")),
            },
            (scene::LINE, None, _) => (None, "; speaker: the scene has no quest".into()),
            (scene::LINE, _, None) => (None, "; speaker: the action is not read".into()),
            _ => (
                None,
                "; speaker: the player, then the alias's answer".into(),
            ),
        };
        Line {
            scene: sk.clone(),
            phase,
            action,
            speaker,
            seconds,
            topic: a.and_then(|a| a.topic.as_ref()).map(tl::norm),
            how: format!("{how}{said}"),
            text: None,
        }
    }

    /// The timeline: the caller's track, the actors' places, everything
    /// observed, labels and line texts. `strings` reads the texts (the
    /// mounted archives and the language); without it no text is read.
    /// Reads only.
    pub fn finish(
        &self,
        vm: &Vm<'_>,
        store: &Store,
        strings: Option<(&fo4_vfs::Vfs, &str)>,
        b: &Budget,
    ) -> Timeline {
        let w = &vm.world;
        let mut t = Timeline {
            notes: NOTES.iter().map(|s| s.to_string()).collect(),
            ..Timeline::default()
        };
        // The caller's track, with the cell of each point.
        let mut grids: BTreeMap<FormKey, BTreeMap<[i32; 2], Option<FormKey>>> = BTreeMap::new();
        if let Some(track) = &w.place.track {
            for p in &track.points {
                let space = tl::norm(&p.space);
                let cell = match store.signature(&space) {
                    Some(s) if s == *b"CELL" => Some(space.clone()),
                    Some(s) if s == *b"WRLD" => {
                        let g = grids
                            .entry(space.clone())
                            .or_insert_with(|| world_cells(store, &space, b));
                        let at = [
                            (p.pos[0] / tl::EXTERIOR_CELL_UNITS).floor() as i32,
                            (p.pos[1] / tl::EXTERIOR_CELL_UNITS).floor() as i32,
                        ];
                        g.get(&at).cloned().flatten()
                    }
                    _ => None,
                };
                t.events.push(Event {
                    t: p.t,
                    what: What::Player {
                        space,
                        cell,
                        pos: p.pos,
                        facing: p.facing,
                    },
                });
            }
        }
        // Stored actor positions, then the moves applied so far.
        for a in &w.place.actors {
            t.events.push(Event {
                t: 0.0,
                what: What::Actor {
                    reference: tl::norm(&a.key),
                    at: Spot::At {
                        space: tl::norm(&a.space),
                        pos: a.pos,
                    },
                    source: crate::rules::WORLD_ACTOR_POSITION.into(),
                },
            });
        }
        let applied = w.place.actor_cursor.min(w.place.actor_moves.len());
        for m in &w.place.actor_moves[..applied] {
            let package =
                self.package_moves.contains(m) || w.place.package_moves.iter().any(|(_, x)| x == m);
            t.events.push(Event {
                t: m.t,
                what: What::Actor {
                    reference: tl::norm(&m.actor),
                    at: Spot::At {
                        space: tl::norm(&m.space),
                        pos: m.pos,
                    },
                    source: if package {
                        crate::rules::PACKAGE_DESTINATION.into()
                    } else {
                        "caller assumption --assume-actor-at (or a package destination applied before the writer looked)".into()
                    },
                },
            });
        }
        for m in &self.moves {
            let at = match &m.to {
                Some((space, pos)) => Spot::At {
                    space: space.clone(),
                    pos: *pos,
                },
                None => Spot::Open("a native of the run may have moved it; where is open".into()),
            };
            let source = crate::rules::WORLD_MOVETO_TARGET.to_string();
            let what = if store.signature(&m.key) == Some(*b"ACHR") {
                What::Actor {
                    reference: m.key.clone(),
                    at,
                    source,
                }
            } else {
                What::Move {
                    reference: m.key.clone(),
                    at,
                    source,
                }
            };
            t.events.push(Event { t: m.t, what });
        }
        t.events.extend(self.events.iter().cloned());
        // Lines, with their text where the data settles it.
        let topics = strings.map(|_| crate::voice::Topics::read(store));
        let mut cache = strings.map(|_| fo4_plugin::StringCache::new(b));
        for s in &self.lines {
            let mut line = s.line.clone();
            if let (Some((vfs, language)), Some(topics), Some(cache), Some(topic), scene::LINE) = (
                strings,
                topics.as_ref(),
                cache.as_mut(),
                line.topic.clone(),
                s.kind,
            ) {
                match topics.of(&topic) {
                    [one] => match line_text(store, vfs, language, cache, one, b) {
                        Ok(text) => line.text = Some(text),
                        Err(why) => line.how.push_str(&format!("; text: {why}")),
                    },
                    [] => line.how.push_str("; text: the topic holds no line"),
                    many => line.how.push_str(&format!(
                        "; text: the topic holds {} lines; which plays is not recorded",
                        many.len()
                    )),
                }
            }
            t.events.push(Event {
                t: s.t,
                what: What::Line(line),
            });
        }
        t.sort();
        // Labels: the editor id of every record the events name.
        let mut keys: BTreeSet<FormKey> = BTreeSet::new();
        for e in &t.events {
            match &e.what {
                What::Player { space, cell, .. } => {
                    keys.insert(space.clone());
                    keys.extend(cell.clone());
                }
                What::Actor { reference, .. }
                | What::Visibility { reference, .. }
                | What::Move { reference, .. } => {
                    keys.insert(reference.clone());
                }
                What::Stage { quest, .. }
                | What::Objective { quest, .. }
                | What::Quest { quest, .. } => {
                    keys.insert(quest.clone());
                }
                What::Scene { scene, .. } => {
                    keys.insert(scene.clone());
                }
                What::Line(l) => {
                    keys.insert(l.scene.clone());
                    keys.extend(l.speaker.clone());
                    keys.extend(l.topic.clone());
                }
                What::Note(_) => {}
            }
        }
        for k in keys {
            let edid = store
                .raw_with_plugin(&k, &b.child())
                .ok()
                .flatten()
                .and_then(|(r, _)| bind::editor_id(&r));
            if let Some(l) = edid {
                t.labels.insert(k, l);
            }
        }
        t
    }
}

/// The exterior cells of a worldspace by grid square (one scan of the
/// store's cells); a square two cells claim maps to `None`.
fn world_cells(store: &Store, world: &FormKey, b: &Budget) -> BTreeMap<[i32; 2], Option<FormKey>> {
    let mut out: BTreeMap<[i32; 2], Option<FormKey>> = BTreeMap::new();
    for c in store.cells() {
        if store.world(&c).as_ref().map(tl::norm).as_ref() != Some(world) {
            continue;
        }
        let Ok(Some(g)) = fo4_scene::grid::grid_of(store, &c, &b.child()) else {
            continue;
        };
        out.entry(g)
            .and_modify(|x| *x = None)
            .or_insert(Some(tl::norm(&c)));
    }
    out
}

/// The text of a line (INFO): its responses' NAM1 in stored order, joined
/// by a space; a localized plugin's string id is looked up in the plugin's
/// three tables and used only where exactly one holds it.
fn line_text(
    store: &Store,
    vfs: &fo4_vfs::Vfs,
    language: &str,
    cache: &mut fo4_plugin::StringCache,
    line: &FormKey,
    b: &Budget,
) -> Result<String, String> {
    let (r, plugin) = store
        .raw_with_plugin(line, &b.child())
        .ok()
        .flatten()
        .ok_or("the line does not read")?;
    let header = &store.order().headers()[plugin];
    let localized = header.flags & fo4_plugin::LOCALIZED != 0;
    let mut parts = Vec::new();
    for s in r.subrecords.iter().filter(|s| s.signature == *b"NAM1") {
        let d = r.data(s);
        if localized {
            let id: [u8; 4] = d.try_into().map_err(|_| "a NAM1 that is not four bytes")?;
            let id = u32::from_le_bytes(id);
            if id == 0 {
                continue;
            }
            let mut found = Vec::new();
            for kind in [
                fo4_plugin::StringKind::Strings,
                fo4_plugin::StringKind::DlStrings,
                fo4_plugin::StringKind::IlStrings,
            ] {
                if let Ok(text) = cache.resolve(vfs, &header.name, language, kind, id) {
                    found.push(text.to_string());
                }
            }
            match found.as_slice() {
                [one] => parts.push(one.clone()),
                [] => return Err(format!("string {id:#x} is in none of the tables")),
                _ => return Err(format!("string {id:#x} is in several tables")),
            }
        } else {
            let d = d.strip_suffix(&[0]).unwrap_or(d);
            parts.push(String::from_utf8_lossy(d).into_owned());
        }
    }
    if parts.is_empty() {
        return Err("the line holds no response text".into());
    }
    Ok(parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MapLibrary, NoHost};

    fn key(id: u32) -> String {
        format!("base.esm:{id:08X}")
    }

    /// Changes made to a run's world between looks are kept with the
    /// second of the look that saw them, and a look changes nothing.
    #[test]
    fn the_recorder_stamps_what_the_logs_grew_by_with_the_second_it_saw_them() {
        let mut lib = MapLibrary::default();
        let mut host = NoHost;
        let mut vm = Vm::new(&mut lib, &mut host);
        let q = vm.world.keyed_object("QUST q", &key(0x20), "quest");
        let r = vm
            .world
            .keyed_object("REFR r", &key(0x40), "objectreference");
        let alias = vm
            .world
            .keyed_object("alias", "base.esm:00000020#alias:3", "referencealias");
        let effect = |native: &str, o: ObjId| crate::Effect {
            depth: 0,
            caller: "x.y".into(),
            native: native.into(),
            receiver: Some("r".into()),
            receiver_object: Some(o),
            args: vec![],
            result: "none".into(),
        };
        let mut rec = Recorder::new();
        rec.observe(&vm);
        vm.world.quests.log.push((q, QuestChange::Started));
        vm.world.quests.log.push((q, QuestChange::StageSet(10)));
        vm.effects.push(effect("objectreference.disable", r));
        vm.world.now[Clock::Real as usize] = 2.5;
        rec.observe(&vm);
        vm.effects.push(effect("objectreference.enablenowait", r));
        vm.effects.push(effect("objectreference.enable", alias));
        vm.world
            .quests
            .log
            .push((q, QuestChange::Objective(quest::DISPLAYED, 5, Some(true))));
        vm.world.place.native_at.insert(
            r,
            (
                3.0,
                FormKey {
                    plugin: "base.esm".into(),
                    object_id: 0x10,
                },
                [1., 2., 3.],
            ),
        );
        vm.world.place.moved_at.insert(r, 3.0);
        vm.world.now[Clock::Real as usize] = 4.0;
        let before = (vm.effects.len(), vm.world.quests.log.len(), vm.rules.len());
        rec.observe(&vm);
        // Seeing the same logs again adds nothing.
        rec.observe(&vm);
        assert_eq!(
            before,
            (vm.effects.len(), vm.world.quests.log.len(), vm.rules.len())
        );
        assert_eq!(rec.observations, 4);
        let k = |id: u32| FormKey {
            plugin: "base.esm".into(),
            object_id: id,
        };
        let stamped: Vec<(f64, &What)> = rec.events.iter().map(|e| (e.t, &e.what)).collect();
        assert_eq!(
            stamped,
            vec![
                (
                    2.5,
                    &What::Visibility {
                        reference: k(0x40),
                        shown: false,
                        nowait: false
                    }
                ),
                (
                    2.5,
                    &What::Quest {
                        quest: k(0x20),
                        change: "Started".into()
                    }
                ),
                (
                    2.5,
                    &What::Stage {
                        quest: k(0x20),
                        stage: 10
                    }
                ),
                (
                    4.0,
                    &What::Visibility {
                        reference: k(0x40),
                        shown: true,
                        nowait: true
                    }
                ),
                (
                    4.0,
                    &What::Note("objectreference.enable on r: not a placed record".into())
                ),
                (
                    4.0,
                    &What::Objective {
                        quest: k(0x20),
                        objective: 5,
                        kind: ObjectiveKind::Displayed,
                        value: Some(true)
                    }
                ),
            ]
        );
        // The native's move keeps its own second.
        assert_eq!(
            rec.moves,
            vec![NativeMove {
                t: 3.0,
                key: k(0x40),
                to: Some((k(0x10), [1., 2., 3.]))
            }]
        );
    }
}
