//! The window's driver: the run on its own thread, its input arriving on a
//! channel slice by slice (the window's walker and activation key, one
//! slice per frame), and what the window shows sent back each time the run
//! asks for input: the run's second, the subtitles of the lines being
//! spoken (from the run's timeline, `--timeline`), the last stage set and
//! the references the run disabled (which the window's activation passes
//! through).
use crate::state::{current_objective, spoken_notes, spoken_texts, subtitles, Texts, Visibility};
use crate::{Driver, Next, Slice, View};
use fo4_plugin::FormKey;
use fo4_scene::timeline::Timeline;
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::JoinHandle;

/// What the window sends the run.
#[derive(Debug, Clone, PartialEq)]
pub enum ToRun {
    Slice(Slice),
    /// The input is over; the run goes on to its end.
    Finish,
    /// The window closed: the run ends now and writes its outputs.
    Stop,
}

/// What the run sends back each time it asks for input.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    /// The run's present second.
    pub now: f64,
    /// The subtitles of the lines being spoken at that second.
    pub subtitles: Vec<String>,
    /// Of those, the texts the data settles (what a viewport shows), and
    /// what the timeline says about the others (a title bar's notes).
    pub texts: Vec<String>,
    pub notes: Vec<String>,
    /// The objective the run displayed last and has not completed or
    /// failed since: the quest's own objective text, when the record holds
    /// one that is not empty, else the quest's name
    /// ([`crate::state::Texts::objective_line`]).
    pub objective: Option<String>,
    /// The last stage set: (quest label or key, stage, second).
    pub stage: Option<(String, i32, f64)>,
    /// The run's last enable (`true`) or disable of each reference, when it
    /// changed since the last status.
    pub visibility: Option<BTreeMap<FormKey, bool>>,
    /// The run's visibility changes and moves as the live world reads them
    /// (`crate::live::RunState`), when they changed since the last status.
    pub run: Option<crate::live::RunState>,
}

/// The driver on the run's thread.
pub struct Link {
    rx: Receiver<ToRun>,
    tx: Sender<Status>,
    seen: (usize, usize),
    timeline: Option<Timeline>,
    visibility: Visibility,
    /// Statuses that carried the visibility.
    sent_visibility: usize,
    run: crate::live::RunState,
    /// Objective texts read so far.
    objectives: BTreeMap<(FormKey, i32), Option<String>>,
}

impl Link {
    pub fn new(rx: Receiver<ToRun>, tx: Sender<Status>) -> Link {
        Link {
            rx,
            tx,
            seen: (usize::MAX, usize::MAX),
            timeline: None,
            visibility: Visibility::default(),
            sent_visibility: 0,
            run: crate::live::RunState::default(),
            objectives: BTreeMap::new(),
        }
    }

    /// The status at the run's present second; the timeline is read again
    /// only when the run's quest or scene log grew.
    fn status(&mut self, run: &View<'_, '_>) -> Status {
        let w = &run.vm.world;
        let logs = (w.quests.log.len(), w.scenes.log.len());
        if logs != self.seen {
            self.seen = logs;
            self.timeline = run.timeline();
        }
        let changed = self.visibility.update(run.vm) || self.sent_visibility == 0;
        let moved = self.run.update(run.vm);
        let now = run.now();
        let mut status = Status {
            now,
            ..Status::default()
        };
        if let Some(t) = &self.timeline {
            let f = t.at(now);
            status.subtitles = subtitles(&f);
            status.texts = spoken_texts(&f);
            status.notes = spoken_notes(&f);
            if let Some(k) = current_objective(&f) {
                if !self.objectives.contains_key(&k) {
                    let (m, vfs) = run.sources;
                    let b = fo4_core::Budget::new(m.limits.max_memory);
                    let text =
                        Texts::new(vfs, &m.language, &b).objective_line(run.store, &k.0, k.1);
                    self.objectives.insert(k.clone(), text);
                }
                status.objective = self.objectives.get(&k).cloned().flatten();
            }
            status.stage = f.last_stage.map(|(q, n, s)| {
                let label = t
                    .labels
                    .get(&q)
                    .cloned()
                    .unwrap_or_else(|| format!("{}:{:08X}", q.plugin, q.object_id));
                (label, n, s)
            });
        }
        if changed {
            self.sent_visibility += 1;
            status.visibility = Some(self.visibility.state.clone());
        }
        if moved {
            status.run = Some(self.run.clone());
        }
        status
    }
}

impl Driver for Link {
    fn next(&mut self, run: &View<'_, '_>) -> Next {
        let s = self.status(run);
        let _ = self.tx.send(s);
        match self.rx.recv() {
            Ok(ToRun::Slice(s)) => Next::Slice(s),
            Ok(ToRun::Finish) => Next::Finish,
            Ok(ToRun::Stop) | Err(_) => Next::Stop,
        }
    }
}

/// The run's thread: the play counts, or why the run failed.
pub type RunThread = JoinHandle<Result<BTreeMap<String, u64>, String>>;

/// Starts `quest_run` with these arguments (`MANIFEST new-game OUT_DIR
/// [switches...]`) on its own thread, driven by the channel.
pub fn spawn(args: Vec<String>) -> std::io::Result<(Sender<ToRun>, Receiver<Status>, RunThread)> {
    let (to_run, from_window) = channel();
    let (to_window, status) = channel();
    let handle = std::thread::Builder::new()
        .name("fo4-play run".into())
        .spawn(move || {
            let link = Link::new(from_window, to_window);
            crate::new_game(args, Some(Box::new(link)))
                .map(|(_, counts)| counts)
                .map_err(|e| e.to_string())
        })?;
    Ok((to_run, status, handle))
}

/// The window's side of the loop, without the window: the play clock (the
/// sum of the frames' seconds, `play_run_clock_input_seconds_v1`) and the
/// walker's feet each frame made into slices as [`crate::walker::Walker`]
/// makes them (`play_player_point_walker_feet_v1`).
#[derive(Debug, Clone)]
pub struct Frames {
    pub t: f64,
    space: FormKey,
    last: crate::Point,
}

impl Frames {
    /// From the run's first point (the walker's start): the window's walker
    /// stands there at second 0.
    pub fn new(first: crate::Point) -> Frames {
        Frames {
            t: first.t,
            space: first.space.clone(),
            last: first,
        }
    }

    /// The feet of the last point given.
    pub fn last(&self) -> [f64; 3] {
        self.last.pos
    }

    /// One frame of `dt` seconds after which the walker's feet are at
    /// `feet`; `activated`: the reference the activation key met this
    /// frame (at the frame's end).
    pub fn frame(&mut self, dt: f64, feet: [f64; 3], activated: Option<FormKey>) -> Slice {
        let t0 = self.t;
        if dt.is_finite() && dt > 0.0 {
            self.t += dt;
        }
        let mut points = Vec::new();
        let moved = fo4_walk::geometry::length(fo4_walk::geometry::sub(feet, self.last.pos));
        if moved > crate::walker::STILL && self.t > t0 {
            if self.last.t < t0 {
                points.push(crate::Point {
                    t: t0,
                    ..self.last.clone()
                });
            }
            let p = crate::Point {
                t: self.t,
                space: self.space.clone(),
                pos: feet,
                facing: None,
            };
            points.push(p.clone());
            self.last = p;
        }
        let actions = activated
            .map(|reference| crate::Act {
                at: self.t,
                action: crate::PlayerAction::Activate,
                reference,
            })
            .into_iter()
            .collect();
        Slice {
            points,
            until: self.t,
            actions,
        }
    }
}

/// The window's title line from the last status.
pub fn title(s: &Status) -> String {
    title_at(s, s.now)
}

/// [`title`] with the play clock's second in place of the run's last
/// event's (a window's title follows the clock the walker goes by).
pub fn title_at(s: &Status, clock: f64) -> String {
    let mut t = format!("fo4-view play | run {clock:.1} s");
    if let Some((q, n, at)) = &s.stage {
        t.push_str(&format!(" | {q} stage {n} at {at:.1} s"));
    }
    for line in &s.subtitles {
        t.push_str(" | ");
        t.push_str(line);
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u32) -> FormKey {
        FormKey {
            plugin: "a.esm".into(),
            object_id: n,
        }
    }

    #[test]
    fn frames_give_points_only_where_the_walker_moved() {
        let first = crate::Point {
            t: 0.0,
            space: key(1),
            pos: [0.0; 3],
            facing: None,
        };
        let mut f = Frames::new(first);
        // Two still frames, then a move, a still frame and an activation.
        let s = f.frame(0.5, [0.0; 3], None);
        assert!(s.points.is_empty() && s.until == 0.5);
        let s = f.frame(0.5, [0.0; 3], None);
        assert!(s.points.is_empty() && s.until == 1.0);
        let s = f.frame(0.25, [10.0, 0.0, 0.0], None);
        // The stand's end at 1 s, then the move's end.
        assert_eq!(s.points.len(), 2);
        assert_eq!((s.points[0].t, s.points[0].pos[0]), (1.0, 0.0));
        assert_eq!((s.points[1].t, s.points[1].pos[0]), (1.25, 10.0));
        let s = f.frame(0.25, [10.0, 0.0, 0.0], Some(key(9)));
        assert!(s.points.is_empty());
        assert_eq!(s.actions.len(), 1);
        assert_eq!((s.actions[0].at, &s.actions[0].reference), (1.5, &key(9)));
        // A frame of no time adds no second.
        let s = f.frame(0.0, [20.0, 0.0, 0.0], None);
        assert!(s.points.is_empty() && s.until == 1.5);
    }

    #[test]
    fn the_title_carries_the_stage_and_the_subtitles() {
        let s = Status {
            now: 12.34,
            subtitles: vec!["Hello.".into()],
            stage: Some(("Q".into(), 20, 10.0)),
            visibility: None,
            run: None,
            ..Status::default()
        };
        assert_eq!(
            title(&s),
            "fo4-view play | run 12.3 s | Q stage 20 at 10.0 s | Hello."
        );
        // The play clock in place of the run's last event's second.
        assert_eq!(
            title_at(&s, 15.06),
            "fo4-view play | run 15.1 s | Q stage 20 at 10.0 s | Hello."
        );
    }
}
