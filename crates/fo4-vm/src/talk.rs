//! Dialogue outside scenes (`spec/vm.md`, Dialogue outside scenes): the
//! lines of topics no scene action names, spoken when what triggers them
//! happens, each step a pending rule behind its own switch, off by
//! default.
//!
//! - A conversation's parties (`dialogue_conversation_parties_v1`): a
//!   line's conditions run on the speaker (run-on 0) and on the one spoken
//!   to (run-on 1); a line is tried with either party speaking.
//! - Which line plays (`dialogue_outside_line_choice_v1`): the candidates
//!   are the lines of the topics of one subtype (DIAL `SNAM`) whose owning
//!   quest (`QNAM`) runs; the line is known when exactly one line, with
//!   one of the two speakers, holds and nothing else is undetermined. The
//!   alternative reading (`dialogue_line_priority_v1`) orders the lines by
//!   their quest's priority, the topic's priority and stored order: the
//!   first that holds plays when every line before it is false.
//! - Greetings (`dialogue_greeting_on_talk_v1`): a greeting topic's line
//!   (`GREE`) when the player starts a conversation with an actor, which
//!   only a caller assumes (`quest_run --assume-talk`).
//! - Hellos (`dialogue_hello_proximity_v1`): a hello topic's line (`HELO`)
//!   when the player's track comes within the hello distance of an enabled
//!   placed actor at its stored position (`crate::track`).
//! - A line's stage fields (`dialogue_line_stage_fields_v1`): its TIQS
//!   halves set on its owning quest as it begins and as it ends, its
//!   seconds from its audio (`crate::voice`) or the caller's assumed line
//!   seconds.
//!
//! The steps run as synthetic calls of the quest host ([`CALL`]), as the
//! scene steps do, so that the stages they set queue their jobs.
use crate::{
    bind,
    cond::{self, Combine, Truth},
    data::{key_record, record_object},
    host::{Host, NativeCall, Reply},
    quest::{self, QuestHost, RunFacts},
    rules, scene,
    track::{self, Shape, Volume},
    voice::{self, LineTime, Speaker},
    world::{Clock, ObjId, Val, World},
    Vm,
};
use fo4_core::Budget;
use fo4_plugin::{
    actor::Context, condition::Condition, dial::Topic, info::Info, typed::Store, FormKey,
};
use fo4_scene::transform::Transform;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// The subtype code of greeting topics (`dialogue_greeting_on_talk_v1`).
pub const GREETING: [u8; 4] = *b"GREE";
/// The subtype code of hello topics (`dialogue_hello_proximity_v1`).
pub const HELLO: [u8; 4] = *b"HELO";
/// The script name of the synthetic calls.
pub const CALL: &str = "::talk";

/// A party of a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Party {
    Player,
    /// A placed actor.
    Actor(FormKey),
}

/// Who speaks and who is spoken to, while a line's conditions are tried
/// (`dialogue_conversation_parties_v1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parties {
    pub speaker: Party,
    pub listener: Party,
}

/// The switches of this model, each off by default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Switches {
    /// `--greeting-on-talk` (`dialogue_greeting_on_talk_v1`).
    pub greeting: bool,
    /// `--hello-lines` (`dialogue_hello_proximity_v1`).
    pub hello: bool,
    /// `--dialogue-priority` (`dialogue_line_priority_v1`).
    pub priority: bool,
    /// `--line-stage-fields` (`dialogue_line_stage_fields_v1` for lines
    /// outside scenes).
    pub stage_fields: bool,
}

/// One line of a topic of a subtype the model reads.
#[derive(Debug, Clone)]
pub struct Line {
    pub key: FormKey,
    pub topic: FormKey,
    /// The topic's QNAM.
    pub quest: Option<FormKey>,
    /// The topic's PNAM.
    pub topic_priority: f32,
    pub plugin: usize,
    pub file: String,
    pub conditions: Vec<Condition>,
    /// Position in stored order (plugin order, then file order).
    pub order: usize,
}

/// The lines of the subtypes the model reads, by subtype code.
#[derive(Debug, Default)]
pub struct Lines {
    pub by_subtype: BTreeMap<[u8; 4], Vec<Line>>,
    /// Topics or lines that did not read.
    pub unread: usize,
}
impl Lines {
    /// Reads the topics of the given subtypes and their lines.
    pub fn read(store: &Store, subtypes: &[[u8; 4]], b: &Budget) -> Lines {
        let mut out = Lines::default();
        let mut topics: BTreeMap<String, ([u8; 4], Option<FormKey>, f32)> = BTreeMap::new();
        for w in store.winners() {
            if w.signature != *b"DIAL" || w.deleted {
                continue;
            }
            let k = store.overrides().form_key(w.key, store.order());
            let bb = b.child();
            let Ok(Some((r, plugin))) = store.raw_with_plugin(&k, &bb) else {
                out.unread += 1;
                continue;
            };
            let ctx = Context {
                order: store.order(),
                plugin,
            };
            let Ok(t) = Topic::read(&r, ctx, &bb) else {
                out.unread += 1;
                continue;
            };
            let Some(sub) = t.subtype.filter(|s| subtypes.contains(s)) else {
                continue;
            };
            topics.insert(
                bind::record_key(&k),
                (sub, t.quest.and_then(|q| q.key), t.priority.unwrap_or(0.0)),
            );
        }
        let mut order = 0;
        for (o, topic) in store.topic_children() {
            if o.signature != *b"INFO" || o.deleted {
                continue;
            }
            order += 1;
            let Some((sub, quest, prio)) = topics.get(&bind::record_key(&topic)).cloned() else {
                continue;
            };
            let k = store.overrides().form_key(o.key, store.order());
            let bb = b.child();
            let Ok(Some((r, plugin))) = store.raw_with_plugin(&k, &bb) else {
                out.unread += 1;
                continue;
            };
            let ctx = Context {
                order: store.order(),
                plugin,
            };
            let Ok(info) = Info::read(&r, ctx, &bb) else {
                out.unread += 1;
                continue;
            };
            out.by_subtype.entry(sub).or_default().push(Line {
                key: k,
                topic,
                quest,
                topic_priority: prio,
                plugin,
                file: r.source.file.clone(),
                conditions: info.conditions,
                order,
            });
        }
        out
    }
}

/// A line's TIQS half due later on the real-time clock.
#[derive(Debug, Clone, PartialEq)]
pub struct Half {
    pub due: f64,
    pub quest: FormKey,
    pub stage: i16,
    /// The line, as a record key.
    pub line: String,
}

/// One change of the player's hello distance from a placed actor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HelloCrossing {
    pub t: f64,
    /// Index in `crate::track::Place::actors`.
    pub actor: usize,
    pub enter: bool,
}

/// The model's state in a run's world; off unless a caller turns a
/// switch on.
#[derive(Debug, Default)]
pub struct Talk {
    pub on: bool,
    pub sw: Switches,
    /// The NPC_ the caller names as the player's base (`quest_run
    /// --player-base`, a caller assumption).
    pub player_base: Option<FormKey>,
    /// The hello distance in units (`dialogue_hello_proximity_v1`, the
    /// value of the game setting the caller names).
    pub hello_distance: Option<f64>,
    /// Caller assumption (`quest_run --assume-line`): where the records
    /// leave the line undetermined, one of these lines plays when its own
    /// conditions hold.
    pub assume_lines: BTreeSet<FormKey>,
    pub lines: Option<Rc<Lines>>,
    pub halves: Vec<Half>,
    /// The player's crossings of each placed actor's hello distance,
    /// earliest first, and how many were applied.
    pub crossings: Vec<HelloCrossing>,
    pub cursor: usize,
    /// What happened, by second, as text (for the names file).
    pub log: Vec<(f64, String)>,
    /// Counts by text.
    pub counts: BTreeMap<String, u64>,
}
impl Talk {
    pub fn count(&mut self, k: impl Into<String>) {
        *self.counts.entry(k.into()).or_default() += 1;
    }
}

/// The player's crossings of the hello distance about every placed actor
/// near the track at its stored position (`dialogue_hello_proximity_v1`):
/// the track's moves against a sphere of that radius, the player's
/// reference point.
pub fn load_hello(w: &mut World) {
    let (Some(d), Some(track)) = (w.talk.hello_distance, w.place.track.clone()) else {
        return;
    };
    if !w.talk.sw.hello || !w.place.sw.actors {
        return;
    }
    let mut out = Vec::new();
    for (i, a) in w.place.actors.iter().enumerate() {
        let Ok(frame) = Transform::euler(
            [
                a.pos[0] as f32,
                a.pos[1] as f32,
                a.pos[2] as f32,
                0.0,
                0.0,
                0.0,
            ],
            1.0,
        ) else {
            continue;
        };
        let v = Volume {
            key: a.key.clone(),
            space: a.space.clone(),
            frame,
            shape: Shape::Sphere(d),
        };
        for (e, l) in track::intervals(&track, &v, None) {
            out.push(HelloCrossing {
                t: e,
                actor: i,
                enter: true,
            });
            if l.is_finite() {
                out.push(HelloCrossing {
                    t: l,
                    actor: i,
                    enter: false,
                });
            }
        }
    }
    out.sort_by(|a, b| {
        a.t.total_cmp(&b.t)
            .then(a.enter.cmp(&b.enter))
            .then(a.actor.cmp(&b.actor))
    });
    w.talk.crossings = out;
}

/// The earliest second after the run's present at which the model has
/// something due: a TIQS half or a hello crossing.
pub fn next_due(w: &World) -> Option<f64> {
    if !w.talk.on {
        return None;
    }
    let now = w.now[Clock::Real as usize];
    w.talk
        .halves
        .iter()
        .map(|h| h.due)
        .chain(w.talk.crossings.get(w.talk.cursor).map(|c| c.t))
        .filter(|t| *t > now)
        .min_by(f64::total_cmp)
}

fn call_of(name: &str, receiver: Option<Val>) -> NativeCall {
    NativeCall {
        script: CALL.into(),
        name: name.into(),
        receiver,
        instance: None,
        args: Vec::new(),
        param_types: Vec::new(),
        return_type: "none".into(),
    }
}

/// The synthetic call of the player starting a conversation with an
/// actor (`--assume-talk`).
pub fn talk_call(actor: ObjId) -> NativeCall {
    call_of("greet", Some(Val::Object(actor)))
}

fn host_call(vm: &mut Vm<'_>, c: &NativeCall) {
    let _ = vm.host.call(c, &mut vm.world);
    for r in vm.host.rules() {
        vm.rules.insert(r);
    }
}

/// Applies what is due at the run's present second: the TIQS halves due,
/// then the hello crossings reached (a hello line on each entry of an
/// actor at its stored position), running the jobs each brings. Returns
/// the job runs.
pub fn settle(vm: &mut Vm<'_>, store: &Store, max_jobs: usize, b: &Budget) -> Vec<quest::JobRun> {
    let mut runs = Vec::new();
    if !vm.world.talk.on {
        return runs;
    }
    let now = vm.world.now[Clock::Real as usize];
    if vm.world.talk.halves.iter().any(|h| h.due <= now) {
        host_call(vm, &call_of("halves", None));
        runs.extend(quest::run_jobs(vm, store, max_jobs, b));
    }
    while let Some(c) = vm.world.talk.crossings.get(vm.world.talk.cursor).copied() {
        if c.t > now {
            break;
        }
        vm.world.talk.cursor += 1;
        if !c.enter {
            continue;
        }
        let a = vm.world.place.actors[c.actor].clone();
        let Ok(o) = record_object(&mut vm.world, store, &a.key, "actor", b) else {
            continue;
        };
        // The actor must still stand where it was placed.
        let here = track::spot(
            store,
            store.limits.max_memory,
            &mut vm.world,
            Some(&Val::Object(o)),
        );
        let at_place = matches!(&here, track::Spot::At { space, pos, .. } if *space == a.space && *pos == a.pos);
        if !at_place {
            vm.world
                .talk
                .count("hello distance entered: the actor is not at its stored position (no line)");
            continue;
        }
        vm.world.talk.count("hello distance entered");
        host_call(vm, &call_of("hello", Some(Val::Object(o))));
        runs.extend(quest::run_jobs(vm, store, max_jobs, b));
    }
    runs
}

/// The talk steps, answered by the quest host.
pub(crate) fn call(h: &mut QuestHost<'_>, c: &NativeCall, w: &mut World) -> Reply {
    let done = Reply::Value(Val::None);
    match c.name.as_str() {
        "halves" => {
            let now = w.now[Clock::Real as usize];
            let mut due: Vec<Half> = Vec::new();
            w.talk.halves.retain(|x| {
                if x.due <= now {
                    due.push(x.clone());
                    false
                } else {
                    true
                }
            });
            due.sort_by(|a, b| a.due.total_cmp(&b.due));
            for x in due {
                h.using(rules::DIALOGUE_LINE_FIELDS);
                w.talk.count("line stage fields: end half set on the clock");
                w.talk.log.push((
                    now,
                    format!("end half of {} sets stage {}", x.line, x.stage),
                ));
                scene::set_field_stage(h, w, &x.quest, x.stage);
            }
            done
        }
        "greet" | "hello" => {
            let greet = c.name == "greet";
            let (on, what, sub, rule) = if greet {
                (
                    w.talk.sw.greeting,
                    "greeting",
                    GREETING,
                    rules::DIALOGUE_GREETING,
                )
            } else {
                (w.talk.sw.hello, "hello", HELLO, rules::DIALOGUE_HELLO)
            };
            if !on {
                w.talk.count(format!("{what}: no line (its switch is off)"));
                return done;
            }
            let Some(Val::Object(o)) = c.receiver.clone() else {
                return done;
            };
            let Some(actor) = w.objects[o].key.as_deref().and_then(key_record) else {
                w.talk.count(format!("{what}: not a placed actor"));
                return done;
            };
            if h.data.store.signature(&actor) != Some(*b"ACHR") {
                w.talk.count(format!("{what}: not a placed actor"));
                return done;
            }
            match h.reference_disabled(w, &actor) {
                Some(Ok(false)) => {}
                Some(Ok(true)) => {
                    w.talk
                        .count(format!("{what}: the actor is disabled (no line)"));
                    return done;
                }
                // The caller's assumption that the player talks to the
                // actor stands for its being there.
                Some(Err(why)) if greet => {
                    w.talk.count(format!(
                        "{what}: whether the actor is enabled is undetermined ({why}); the caller's assumption stands for it"
                    ));
                }
                Some(Err(why)) => {
                    w.talk.count(format!(
                        "{what}: whether the actor is enabled is undetermined: {why} (no line)"
                    ));
                    return done;
                }
                None => {
                    w.talk.count(format!(
                        "{what}: whether the actor is enabled is undetermined: the host keeps no enable state (no line)"
                    ));
                    return done;
                }
            }
            h.using(rule);
            speak(h, w, sub, what, &actor);
            done
        }
        _ => Reply::Undetermined,
    }
}

/// What trying one line with one speaker gave.
#[derive(Debug, Clone, PartialEq)]
enum Tried {
    Holds,
    Fails,
    Open(BTreeSet<u16>),
}

/// Chooses and plays the line of a subtype in a conversation between the
/// player and `actor`.
fn speak(h: &mut QuestHost<'_>, w: &mut World, sub: [u8; 4], what: &str, actor: &FormKey) {
    let store = h.data.store;
    let lines = match w.talk.lines.clone() {
        Some(l) => l,
        None => {
            let b = Budget::new(h.data.max_memory);
            let l = Rc::new(Lines::read(store, &[GREETING, HELLO], &b));
            w.talk.lines = Some(l.clone());
            l
        }
    };
    let Some(cands) = lines.by_subtype.get(&sub) else {
        w.talk.count(format!("{what}: no topic of the subtype"));
        return;
    };
    h.using(rules::DIALOGUE_PARTIES);
    let pairings = [
        Parties {
            speaker: Party::Actor(actor.clone()),
            listener: Party::Player,
        },
        Parties {
            speaker: Party::Player,
            listener: Party::Actor(actor.clone()),
        },
    ];
    // Whether each topic's quest runs, by quest.
    let mut running: BTreeMap<String, Result<bool, &'static str>> = BTreeMap::new();
    let mut tried: Vec<(usize, usize, Tried, u8)> = Vec::new();
    let mut used = BTreeSet::new();
    for (i, l) in cands.iter().enumerate() {
        let Some(q) = &l.quest else {
            tried.push((i, 0, Tried::Fails, 0));
            continue;
        };
        let qk = bind::record_key(q);
        let info = quest::info_by_key(store, h.data.max_memory, w, q);
        let prio = info.as_ref().and_then(|i| i.priority).unwrap_or(0);
        let run = match running.get(&qk) {
            Some(r) => *r,
            None => {
                let r = match &info {
                    None => Err("the topic's quest does not read"),
                    Some(i) => {
                        let s = w.quests.state(i);
                        if s.open.is_some() {
                            Err("the topic's quest state is undetermined")
                        } else {
                            s.running
                                .ok_or("whether the topic's quest runs is undetermined")
                        }
                    }
                };
                running.insert(qk, r);
                r
            }
        };
        match run {
            Ok(false) => continue,
            Err(_) => {
                tried.push((i, 0, Tried::Open(BTreeSet::new()), prio));
                continue;
            }
            Ok(true) => {}
        }
        for (p, parties) in pairings.iter().enumerate() {
            let ctx = Context {
                order: store.order(),
                plugin: l.plugin,
            };
            let mut facts = RunFacts::of(h, w, None, Some(q.clone()));
            facts.parties = Some(parties.clone());
            let t = cond::eval_list_with(
                &l.conditions,
                ctx,
                &l.file,
                &mut facts,
                Combine::OrGroups,
                &mut used,
            );
            let t = match t {
                Truth::True => Tried::Holds,
                Truth::False => Tried::Fails,
                Truth::Undetermined(why) => Tried::Open(why.iter().map(|x| x.function).collect()),
            };
            tried.push((i, p, t, prio));
        }
    }
    for u in used {
        h.using(u);
    }
    let open: BTreeSet<u16> = tried
        .iter()
        .filter_map(|(_, _, t, _)| match t {
            Tried::Open(f) => Some(f.iter().copied()),
            _ => None,
        })
        .flatten()
        .collect();
    let any_open = tried.iter().any(|t| matches!(t.2, Tried::Open(_)));
    let holds: Vec<&(usize, usize, Tried, u8)> =
        tried.iter().filter(|t| t.2 == Tried::Holds).collect();
    let now = w.now[Clock::Real as usize];
    let chosen = if w.talk.sw.priority {
        h.using(rules::DIALOGUE_LINE_PRIORITY);
        let mut order: Vec<&(usize, usize, Tried, u8)> =
            tried.iter().filter(|t| t.2 != Tried::Fails).collect();
        order.sort_by(|a, b| {
            let (la, lb) = (&cands[a.0], &cands[b.0]);
            b.3.cmp(&a.3)
                .then(lb.topic_priority.total_cmp(&la.topic_priority))
                .then(la.order.cmp(&lb.order))
                .then(a.1.cmp(&b.1))
        });
        match order.first() {
            Some((i, p, Tried::Holds, _)) => {
                w.talk.count(format!(
                    "{what}: line chosen by priority (--dialogue-priority)"
                ));
                Some((*i, *p))
            }
            Some((_, _, Tried::Open(f), _)) => {
                w.talk.count(format!(
                    "{what}: line undetermined: a line ahead by priority is undetermined (function {})",
                    functions(f)
                ));
                None
            }
            _ => {
                w.talk.count(format!("{what}: no line holds"));
                None
            }
        }
    } else {
        h.using(rules::DIALOGUE_LINE_CHOICE);
        match (holds.as_slice(), any_open) {
            ([one], false) => {
                w.talk
                    .count(format!("{what}: line determined: the only line that holds"));
                Some((one.0, one.1))
            }
            ([], false) => {
                w.talk.count(format!("{what}: no line holds"));
                None
            }
            (_, false) => {
                w.talk
                    .count(format!("{what}: line undetermined: several lines hold"));
                None
            }
            (_, true) => {
                w.talk.count(format!(
                    "{what}: line undetermined: whether a line holds is undetermined (function {})",
                    functions(&open)
                ));
                None
            }
        }
    };
    w.talk.log.push((
        now,
        format!(
            "{what} with {}: {} candidate line(s) of running quests, {} hold(s), {} undetermined{}",
            bind::record_key(actor),
            tried.len(),
            holds.len(),
            tried
                .iter()
                .filter(|t| matches!(t.2, Tried::Open(_)))
                .count(),
            if open.is_empty() {
                String::new()
            } else {
                format!(" (function {})", functions(&open))
            }
        ),
    ));
    // The caller's assumed line, where the choice is not settled.
    let chosen = match chosen {
        Some(c) => Some(c),
        None => {
            let a = holds
                .iter()
                .find(|t| w.talk.assume_lines.contains(&cands[t.0].key))
                .map(|t| (t.0, t.1));
            if a.is_some() {
                w.talk.count(format!(
                    "{what}: line assumed from the command line: a named line whose conditions hold (--assume-line)"
                ));
            }
            a
        }
    };
    // The first lines that do not fail, in priority order, for the names
    // file.
    let mut order: Vec<&(usize, usize, Tried, u8)> =
        tried.iter().filter(|t| t.2 != Tried::Fails).collect();
    order.sort_by(|a, b| {
        let (la, lb) = (&cands[a.0], &cands[b.0]);
        b.3.cmp(&a.3)
            .then(lb.topic_priority.total_cmp(&la.topic_priority))
            .then(la.order.cmp(&lb.order))
            .then(a.1.cmp(&b.1))
    });
    for (i, p, t, prio) in order.iter().take(12) {
        let l = &cands[*i];
        w.talk.log.push((
            now,
            format!(
                "    candidate {} topic {} quest {} priority {prio} topic priority {} speaker {}: {}",
                bind::record_key(&l.key),
                bind::record_key(&l.topic),
                l.quest.as_ref().map(bind::record_key).unwrap_or_default(),
                l.topic_priority,
                if *p == 0 { "the actor" } else { "the player" },
                match t {
                    Tried::Holds => "holds".to_string(),
                    Tried::Fails => "fails".to_string(),
                    Tried::Open(f) => format!("undetermined (function {})", functions(f)),
                }
            ),
        ));
    }
    let Some((i, p)) = chosen else { return };
    let line = cands[i].clone();
    let speaker = pairings[p].speaker.clone();
    play(h, w, &line, &speaker, what);
}

fn functions(f: &BTreeSet<u16>) -> String {
    let v: Vec<String> = f.iter().map(|n| n.to_string()).collect();
    v.join(", ")
}

/// Plays a line: its seconds, then its TIQS halves
/// (`dialogue_line_stage_fields_v1`, with `--line-stage-fields`).
fn play(h: &mut QuestHost<'_>, w: &mut World, line: &Line, speaker: &Party, what: &str) {
    let store = h.data.store;
    let b = Budget::new(h.data.max_memory);
    let now = w.now[Clock::Real as usize];
    let who = match speaker {
        Party::Player => "the player".to_string(),
        Party::Actor(k) => bind::record_key(k),
    };
    w.talk.log.push((
        now,
        format!(
            "{what}: {} of topic {} spoken by {who}",
            bind::record_key(&line.key),
            bind::record_key(&line.topic)
        ),
    ));
    w.talk.count(format!(
        "{what}: lines played, spoken by {}",
        match speaker {
            Party::Player => "the player",
            Party::Actor(_) => "the actor",
        }
    ));
    let clock = w.scenes.clock.clone();
    let audio = match &clock.voice {
        None => LineTime::Undetermined("line audio off".into()),
        Some(v) => {
            let sp = match speaker {
                Party::Actor(k) => voice::reference_speaker(store, k, &b),
                Party::Player => {
                    Speaker::Unknown("the player's voice type is not in the records".into())
                }
            };
            match store.raw_with_plugin(&line.key, &b) {
                Ok(Some((r, plugin))) => {
                    let ctx = Context {
                        order: store.order(),
                        plugin,
                    };
                    match Info::read(&r, ctx, &b) {
                        Ok(info) => {
                            let mut used = BTreeSet::new();
                            let t = voice::line_audio(
                                store,
                                v,
                                line.key.clone(),
                                info,
                                &sp,
                                &mut used,
                                &b,
                            );
                            for u in used {
                                h.using(u);
                            }
                            t
                        }
                        Err(_) => LineTime::Undetermined("the line does not read".into()),
                    }
                }
                _ => LineTime::Undetermined("the line does not read".into()),
            }
        }
    };
    let secs = match (audio, clock.assume_line_seconds) {
        (LineTime::Seconds(x), _) => {
            w.talk.count(format!("{what}: line seconds from its audio"));
            Some(x)
        }
        (LineTime::Undetermined(why), Some(x)) => {
            w.talk.count(format!(
                "{what}: line seconds assumed from the command line ({why})"
            ));
            Some(x)
        }
        (LineTime::Undetermined(why), None) => {
            w.talk
                .count(format!("{what}: line seconds undetermined ({why})"));
            None
        }
    };
    if !w.talk.sw.stage_fields {
        return;
    }
    let tiqs = store.raw(&line.key, &b).ok().flatten().and_then(|r| {
        r.subrecords
            .iter()
            .find(|x| x.signature == *b"TIQS")
            .and_then(|x| scene::stage_pair(r.data(x)))
    });
    let Some([b0, b1]) = tiqs.filter(|[x, y]| *x >= 0 || *y >= 0) else {
        w.talk.count(format!(
            "{what}: line stage fields: the line names no stage"
        ));
        return;
    };
    let key = bind::record_key(&line.key);
    let Some(q) = scene::groups(store, w).info_owner.get(&key).cloned() else {
        w.talk.count(format!(
            "{what}: line stage fields: the line has no owning quest"
        ));
        return;
    };
    h.using(rules::DIALOGUE_LINE_FIELDS);
    if b1 >= 0 {
        match secs {
            Some(s) => {
                w.talk
                    .count(format!("{what}: line stage fields: end half scheduled"));
                w.talk.halves.push(Half {
                    due: now + s,
                    quest: q.clone(),
                    stage: b1,
                    line: key.clone(),
                });
            }
            None => w.talk.count(format!(
                "{what}: line stage fields: end half not applied (the line's time is not settled)"
            )),
        }
    }
    if b0 >= 0 {
        w.talk.count(format!(
            "{what}: line stage fields: begin half set as the line begins"
        ));
        w.talk
            .log
            .push((now, format!("begin half of {key} sets stage {b0}")));
        scene::set_field_stage(h, w, &q, b0);
    }
}
