//! The window's play mode (`--play-run`, `spec/play.md`): `quest_run
//! new-game` on its own thread (`fo4_play::link`), fed each frame with the
//! walker's feet, the frame clock and the activation key's target, and the
//! run's second, last stage and spoken lines shown in the title bar. The
//! loop's pieces are tested headless (`fo4_play::link::Frames`, the pick);
//! the window itself has not been opened by anyone (no one can sit at the
//! machine): the play mode's tested path is `play_run`.
#[cfg(feature = "script-event")]
pub use enabled::*;

/// Without the interpreter the play mode cannot start.
#[cfg(not(feature = "script-event"))]
#[derive(Debug)]
pub enum PlayRun {}

#[cfg(not(feature = "script-event"))]
pub fn start(_: &crate::cli::Play, _: &mut crate::cli::Scene) -> Result<PlayRun, String> {
    Err("--play-run needs a viewer built with --features script-event".into())
}

#[cfg(feature = "script-event")]
mod enabled {
    use fo4_play::link::{self, Frames, Status, ToRun};
    use fo4_plugin::FormKey;
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::Mutex;

    /// The window's link to the run: what the frame system needs.
    pub struct PlayRun {
        pub frames: Frames,
        to_run: Sender<ToRun>,
        status: Mutex<Receiver<Status>>,
        thread: Option<link::RunThread>,
        pub last: Status,
        /// The run's last enable or disable of each reference.
        pub visibility: std::collections::BTreeMap<FormKey, bool>,
        pub reach: Option<f64>,
        pub bounds: bool,
        /// The placed reference of each source of the walking world and
        /// the object bounds of those with no collision (from the load).
        pub refs: Vec<Option<FormKey>>,
        pub boxes: fo4_play::pick::Boxes,
        /// Activations sent: (second, what the ray met).
        pub activations: Vec<(f64, String)>,
    }

    /// Reads the switches file, puts the walker at the run's first point
    /// (the eye that far above it, looking the same way) and starts the
    /// run.
    pub fn start(p: &crate::cli::Play, s: &mut crate::cli::Scene) -> Result<PlayRun, String> {
        let text = std::fs::read_to_string(&p.run).map_err(|e| format!("{}: {e}", p.run))?;
        let switches: Vec<String> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect();
        let track = switches
            .iter()
            .position(|x| x == "--track")
            .and_then(|i| switches.get(i + 1))
            .ok_or("PlayNoTrack: the switches need --track FILE (its first point is the start)")?;
        let t = fo4_vm::track::Track::parse(
            &std::fs::read_to_string(track).map_err(|e| format!("{track}: {e}"))?,
        )?;
        let first = t
            .points
            .first()
            .ok_or("PlayNoTrack: an empty track")?
            .clone();
        let eye_height = fo4_walk::Params::PROVISIONAL.eye_height;
        let d: [f64; 3] = std::array::from_fn(|i| s.look[i] - s.eye[i]);
        s.eye = [first.pos[0], first.pos[1], first.pos[2] + eye_height];
        s.look = std::array::from_fn(|i| s.eye[i] + d[i]);
        let mut args = vec![s.manifest.clone(), "new-game".to_string(), p.out.clone()];
        args.extend(switches);
        let (to_run, status, thread) = link::spawn(args).map_err(|e| e.to_string())?;
        println!(
            "play: the run starts at {:?} in {}; E activates within {:?} units",
            first.pos,
            fo4_vm::bind::record_key(&first.space),
            p.reach
        );
        Ok(PlayRun {
            frames: Frames::new(first),
            to_run,
            status: Mutex::new(status),
            thread: Some(thread),
            last: Status::default(),
            visibility: Default::default(),
            reach: p.reach,
            bounds: p.bounds,
            refs: Vec::new(),
            boxes: Default::default(),
            activations: Vec::new(),
        })
    }

    impl PlayRun {
        /// One frame: the slice to the run, then the statuses it sent
        /// since the last frame. Returns the title when it changed.
        pub fn frame(
            &mut self,
            dt: f64,
            feet: [f64; 3],
            activated: Option<FormKey>,
        ) -> Option<String> {
            let slice = self.frames.frame(dt, feet, activated);
            let _ = self.to_run.send(ToRun::Slice(slice));
            let mut changed = false;
            if let Ok(rx) = self.status.lock() {
                while let Ok(s) = rx.try_recv() {
                    if let Some(v) = &s.visibility {
                        self.visibility = v.clone();
                    }
                    changed |= s.subtitles != self.last.subtitles
                        || s.stage != self.last.stage
                        || (s.now - self.last.now).abs() >= 0.1;
                    self.last = s;
                }
            }
            changed.then(|| link::title(&self.last))
        }

        /// What the activation key meets from the eye along `dir`.
        pub fn pick(
            &mut self,
            world: &fo4_walk::World,
            eye: [f64; 3],
            dir: [f64; 3],
        ) -> Option<FormKey> {
            let reach = self.reach?;
            let vis = &self.visibility;
            let hit = fo4_play::pick::first_of(
                world,
                &self.refs,
                self.bounds.then_some(&self.boxes),
                eye,
                dir,
                reach,
                &|k| vis.get(k) == Some(&false),
            );
            let t = self.frames.t;
            let text = match &hit {
                Some(h) => format!("{:?} at {:.1} units", h.reference, h.distance),
                None => "nothing within reach".into(),
            };
            println!("play: activate at {t:.3} s: {text}");
            self.activations.push((t, text));
            hit.and_then(|h| h.reference)
        }
    }

    impl Drop for PlayRun {
        /// The window closed: the run stops, writes its outputs and the
        /// window's process waits for it.
        fn drop(&mut self) {
            let _ = self.to_run.send(ToRun::Stop);
            if let Some(t) = self.thread.take() {
                match t.join() {
                    Ok(Ok(counts)) => {
                        for (k, v) in counts {
                            println!("play: {k}\t{v}");
                        }
                    }
                    Ok(Err(e)) => eprintln!("play: the run failed: {e}"),
                    Err(_) => eprintln!("play: the run's thread panicked"),
                }
            }
        }
    }
}
