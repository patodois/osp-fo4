//! What a spoken line does besides its stage fields (`spec/vm.md`, The
//! walk home): the begin and end fragments of its fragment block
//! (`dialogue_line_fragments_v1`, `quest_run --line-fragments`) and the
//! scene its `TSCE` names, started as it ends at the phase its `INTV`
//! stores (`dialogue_line_start_scene_v1`, `quest_run --line-start-scene`).
//!
//! The lines are those the run already plays: a scene's dialogue action
//! and a player dialogue action under an assumed choice, when the records
//! leave one line (`crate::scene`), and a greeting or hello outside scenes
//! (`crate::talk`); their seconds are those the scene clock or the talk
//! model gives. Both readings are off by default; off, nothing here runs
//! and every output stays as before.
use crate::{
    bind,
    data::record_object,
    quest::{Job, QuestHost},
    rules, scene,
    world::{ObjId, World},
};
use fo4_core::Budget;
use fo4_plugin::{typed::Store, vmad::Vmad, FormKey};

/// The two switches of this module, kept in the scene state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Switches {
    /// `--line-fragments` (`dialogue_line_fragments_v1`).
    pub fragments: bool,
    /// `--line-start-scene` (`dialogue_line_start_scene_v1`).
    pub start_scene: bool,
}

/// What a line's record holds for the two readings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Extras {
    /// The lower-case function names of the fragment block's begin and
    /// end fragments.
    pub begin: Option<String>,
    pub end: Option<String>,
    /// The scene `TSCE` names and the phase `INTV` stores.
    pub scene: Option<FormKey>,
    pub phase: Option<u32>,
}

/// Reads a line's fragment block slots, `TSCE` and `INTV`.
pub fn extras(store: &Store, line: &FormKey, b: &Budget) -> Extras {
    let mut out = Extras::default();
    let Ok(Some((r, plugin))) = store.raw_with_plugin(line, b) else {
        return out;
    };
    for s in &r.subrecords {
        let d = r.data(s);
        match &s.signature {
            b"VMAD" => {
                if let Ok(v) = Vmad::read(d, r.signature, &r.source.file, s.offset, b) {
                    if let Some(block) = &v.info {
                        let lower =
                            |f: &fo4_plugin::vmad::Fragment<'_>| fo4_pex::index::lower(f.function);
                        out.begin = block.slot(fo4_plugin::vmad::FRAGMENT_BEGIN).map(lower);
                        out.end = block.slot(fo4_plugin::vmad::FRAGMENT_END).map(lower);
                    }
                }
            }
            b"TSCE" if d.len() == 4 => {
                let id = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
                out.scene = store
                    .order()
                    .remap(plugin, id, &r.source.file, s.offset)
                    .ok()
                    .flatten()
                    .filter(|k| store.signature(k) == Some(*b"SCEN"));
            }
            b"INTV" if d.len() == 4 => {
                out.phase = Some(u32::from_le_bytes([d[0], d[1], d[2], d[3]]));
            }
            _ => {}
        }
    }
    out
}

/// What a line's end does on the clock.
#[derive(Debug, Clone, PartialEq)]
pub enum EndKind {
    /// Run the end fragment (`dialogue_line_fragments_v1`).
    Fragment {
        function: String,
        slot: String,
        speaker: Option<ObjId>,
    },
    /// Start the scene at a phase (`dialogue_line_start_scene_v1`).
    StartScene { scene: FormKey, phase: Option<u32> },
    /// Deliver the event (`onbegin` or `onend`) to the line's own scripts
    /// (`dialogue_line_script_events_v1`).
    Script {
        event: String,
        speaker: Option<ObjId>,
    },
}

/// Whether a line's record attaches scripts of its own (its VMAD's script
/// list, not the fragment block).
pub fn has_own_scripts(store: &Store, line: &FormKey, b: &Budget) -> bool {
    let Ok(Some(r)) = store.raw(line, b) else {
        return false;
    };
    r.subrecords
        .iter()
        .filter(|s| s.signature == *b"VMAD")
        .any(|s| {
            Vmad::read(r.data(s), r.signature, &r.source.file, s.offset, b)
                .is_ok_and(|v| !v.scripts.is_empty())
        })
}

/// Queues a line's event for its own scripts as a job
/// (`dialogue_line_script_events_v1`).
pub(crate) fn queue_script(
    w: &mut World,
    owner: ObjId,
    line: &FormKey,
    event: &str,
    speaker: Option<ObjId>,
) {
    count(w, format!("line own scripts queued\t{event}"));
    let depth = w.quests.depth + 1;
    w.quests.queue.push_back(Job::Scene {
        scene: owner,
        step: scene::Step::LineScript {
            line: bind::record_key(line),
            event: event.to_string(),
            speaker,
        },
        depth,
    });
}

/// One thing due when a line ends.
#[derive(Debug, Clone, PartialEq)]
pub struct End {
    pub due: f64,
    pub line: FormKey,
    /// The scene handle whose clock plays the line, or the line's own
    /// handle outside scenes.
    pub owner: ObjId,
    pub kind: EndKind,
    /// Whether a job is queued for it.
    pub queued: bool,
}

fn count(w: &mut World, k: impl Into<String>) {
    *w.scenes.line_counts.entry(k.into()).or_default() += 1;
}

/// Queues a line's fragment as a job, on the line's fragment script
/// instance (`vmad_fragment_instance_v1`), with the speaker's reference as
/// its one argument.
pub(crate) fn queue_fragment(
    w: &mut World,
    owner: ObjId,
    line: &FormKey,
    function: String,
    slot: &str,
    speaker: Option<ObjId>,
) {
    count(w, format!("line fragments queued\t{slot}"));
    let depth = w.quests.depth + 1;
    w.quests.queue.push_back(Job::Scene {
        scene: owner,
        step: scene::Step::LineFragment {
            line: bind::record_key(line),
            function,
            slot: slot.to_string(),
            speaker,
        },
        depth,
    });
}

/// What a line the run plays does as it begins and, when its end second is
/// settled, schedules for its end. `begin` is the second it begins, `end`
/// the second it ends when settled.
#[allow(clippy::too_many_arguments)]
pub(crate) fn play(
    h: &mut QuestHost<'_>,
    w: &mut World,
    owner: ObjId,
    line: &FormKey,
    speaker: Option<ObjId>,
    begin_now: bool,
    begin: f64,
    end: Option<f64>,
) {
    let sw = w.scenes.lines;
    if w.scenes.line_scripts {
        let b = Budget::new(h.data.max_memory);
        if has_own_scripts(h.data.store, line, &b) {
            h.using(rules::LINE_SCRIPT_EVENTS);
            if begin_now {
                queue_script(w, owner, line, "onbegin", speaker);
            } else {
                count(w, "line own scripts scheduled\tonbegin");
                w.scenes.line_ends.push(End {
                    due: begin,
                    line: line.clone(),
                    owner,
                    kind: EndKind::Script {
                        event: "onbegin".into(),
                        speaker,
                    },
                    queued: false,
                });
            }
            match end {
                Some(due) => {
                    count(w, "line own scripts scheduled\tonend");
                    w.scenes.line_ends.push(End {
                        due,
                        line: line.clone(),
                        owner,
                        kind: EndKind::Script {
                            event: "onend".into(),
                            speaker,
                        },
                        queued: false,
                    });
                }
                None => count(
                    w,
                    "line own scripts not run\tonend: the line's time is not settled",
                ),
            }
        }
    }
    if !(sw.fragments || sw.start_scene) {
        return;
    }
    let b = Budget::new(h.data.max_memory);
    let x = extras(h.data.store, line, &b);
    if sw.fragments {
        if let Some(f) = x.begin.clone() {
            h.using(rules::LINE_FRAGMENTS);
            if begin_now {
                queue_fragment(w, owner, line, f, "begin", speaker);
            } else {
                count(w, "line fragments scheduled\tbegin");
                w.scenes.line_ends.push(End {
                    due: begin,
                    line: line.clone(),
                    owner,
                    kind: EndKind::Fragment {
                        function: f,
                        slot: "begin".into(),
                        speaker,
                    },
                    queued: false,
                });
            }
        }
        if let Some(f) = x.end.clone() {
            h.using(rules::LINE_FRAGMENTS);
            match end {
                Some(due) => {
                    count(w, "line fragments scheduled\tend");
                    w.scenes.line_ends.push(End {
                        due,
                        line: line.clone(),
                        owner,
                        kind: EndKind::Fragment {
                            function: f,
                            slot: "end".into(),
                            speaker,
                        },
                        queued: false,
                    });
                }
                None => count(
                    w,
                    "line fragments not run\tend: the line's time is not settled",
                ),
            }
        }
    }
    if sw.start_scene {
        if let Some(s) = x.scene.clone() {
            h.using(rules::LINE_START_SCENE);
            match end {
                Some(due) => {
                    count(w, "line start scene scheduled as the line ends");
                    w.scenes.line_ends.push(End {
                        due,
                        line: line.clone(),
                        owner,
                        kind: EndKind::StartScene {
                            scene: s,
                            phase: x.phase,
                        },
                        queued: false,
                    });
                }
                None => count(
                    w,
                    "line start scene not applied: the line's time is not settled",
                ),
            }
        }
    }
}

/// The earliest due second of the line ends not yet queued.
pub fn next_due(w: &World) -> Option<f64> {
    w.scenes
        .line_ends
        .iter()
        .filter(|e| !e.queued)
        .map(|e| e.due)
        .min_by(f64::total_cmp)
}

/// Applies the line ends due at the run's present second, in due order: a
/// fragment is queued as a job; a scene is started (at its phase).
pub(crate) fn apply_due(h: &mut QuestHost<'_>, w: &mut World) {
    let now = w.now[crate::world::Clock::Real as usize];
    let mut due: Vec<End> = Vec::new();
    w.scenes.line_ends.retain(|e| {
        if e.due <= now {
            due.push(e.clone());
            false
        } else {
            true
        }
    });
    due.sort_by(|a, b| a.due.total_cmp(&b.due));
    for e in due {
        match e.kind {
            EndKind::Fragment {
                function,
                slot,
                speaker,
            } => queue_fragment(w, e.owner, &e.line, function, &slot, speaker),
            EndKind::StartScene { scene: s, phase } => start_scene(h, w, &s, phase),
            EndKind::Script { event, speaker } => {
                queue_script(w, e.owner, &e.line, &event, speaker)
            }
        }
    }
}

/// Starts the scene a line names (`dialogue_line_start_scene_v1`): a scene
/// that is not playing starts as `Scene.Start` starts it, offering the
/// phase `INTV` stores first; a playing scene is left as it is (counted).
pub(crate) fn start_scene(h: &mut QuestHost<'_>, w: &mut World, s: &FormKey, phase: Option<u32>) {
    let b = Budget::new(h.data.max_memory);
    let Ok(o) = record_object(w, h.data.store, s, "scene", &b) else {
        count(w, "line start scene: not a scene record");
        return;
    };
    let Some(i) = scene::info(h.data.store, h.data.max_memory, w, o) else {
        count(w, "line start scene: the scene does not read");
        return;
    };
    match scene::start_at(h, w, o, &i, phase.map(|p| p as usize)) {
        Ok(()) => count(
            w,
            format!(
                "line start scene: started{}",
                if phase.is_some_and(|p| p > 0) {
                    " at the stored phase"
                } else {
                    ""
                }
            ),
        ),
        Err(why) => count(w, format!("line start scene: not started\t{why}")),
    }
}
