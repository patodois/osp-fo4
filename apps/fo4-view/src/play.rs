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
impl PlayRun {
    pub fn wants_live(&self) -> bool {
        match *self {}
    }
}

#[cfg(not(feature = "script-event"))]
pub fn start(_: &crate::cli::Play, _: &mut crate::cli::Scene) -> Result<PlayRun, String> {
    Err("--play-run needs a viewer built with --features script-event".into())
}

#[cfg(feature = "script-event")]
mod enabled {
    use fo4_play::link::{self, Frames, Status, ToRun};
    use fo4_play::live::{Decision, Live, Piece, RefInfo, RunState};
    use fo4_plugin::FormKey;
    use std::collections::BTreeMap;
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::Mutex;

    /// What the window's live world needs from the load
    /// (`--play-live-world`).
    pub struct LiveLoad {
        pub dormant: Piece,
        pub info: BTreeMap<FormKey, RefInfo>,
        pub space: FormKey,
        /// The placed reference of each drawn instance tag.
        pub tags: BTreeMap<u32, FormKey>,
        /// The walking world as loaded (a copy of the window's).
        pub world: Option<fo4_walk::World>,
    }

    /// The scene's references the run may enable, drawn for the window to
    /// hide or show: the initially disabled ones loaded and put in after
    /// the instances, the instances an enable parent disables made enabled.
    pub struct Dormant {
        loaded_from: usize,
        forced: Vec<(usize, Option<bool>)>,
    }

    /// Puts the references the run may enable into the scene before its
    /// draw list is built ([`undraw_dormant`] after).
    pub fn draw_dormant(
        scene: &mut fo4_scene::Scene,
        loader: &mut fo4_scene::Loader<'_>,
        budget: &fo4_core::Budget,
    ) -> fo4_core::Result<Dormant> {
        let loaded_from = scene.instances.len();
        let mut forced = Vec::new();
        for (k, i) in scene.instances.iter_mut().enumerate() {
            if i.disabled_by_enable_parent() {
                forced.push((k, i.enable.as_ref().and_then(|p| p.enabled)));
                i.set_enabled(Some(true));
            }
        }
        let keys: Vec<FormKey> = scene.disabled.iter().map(|c| c.key.clone()).collect();
        for k in keys {
            let scope = budget.child();
            if let Ok(i) = loader.instance(&k, &scope) {
                scope.commit(budget)?;
                scene.assets.extend_from_slice(&i.assets);
                scene.instances.push(i);
            }
        }
        scene.assets.sort_unstable();
        scene.assets.dedup();
        println!(
            "play: {} initially disabled references and {} instances an enable parent disables drawn for the live world (hidden until the run enables them)",
            scene.instances.len() - loaded_from,
            forced.len()
        );
        Ok(Dormant {
            loaded_from,
            forced,
        })
    }

    /// Takes the references [`draw_dormant`] put in out again.
    pub fn undraw_dormant(scene: &mut fo4_scene::Scene, d: Dormant) {
        scene.instances.truncate(d.loaded_from);
        for (k, was) in d.forced {
            if let Some(i) = scene.instances.get_mut(k) {
                i.set_enabled(was);
            }
        }
    }
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
        /// `--play-live-world` asked for.
        live_asked: bool,
        nowait: bool,
        /// The live world and the drawn references by instance tag.
        pub live: Option<(Live, BTreeMap<u32, FormKey>)>,
        /// The run's last state, and how many times it was applied.
        state: Option<RunState>,
        pub generation: u64,
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
            live_asked: p.live,
            nowait: p.nowait,
            live: None,
            state: None,
            generation: 0,
        })
    }

    impl PlayRun {
        /// Whether `--play-live-world` was asked for.
        pub fn wants_live(&self) -> bool {
            self.live_asked
        }

        /// The live world from the load: from now on the run's state
        /// changes what the window walks in, picks and draws.
        pub fn follow(&mut self, l: LiveLoad) -> Result<(), String> {
            let world = l.world.ok_or("PlayLiveWorld: no walking world")?;
            let live = Live::new(
                world,
                self.refs.clone(),
                l.dormant,
                l.info,
                l.space,
                self.nowait,
            )
            .map_err(|e| e.to_string())?;
            println!(
                "play: the walking world and the drawn references follow the run (--play-live-world{})",
                if self.nowait { ", --play-nowait" } else { "" }
            );
            self.live = Some((live, l.tags));
            Ok(())
        }

        /// What the live world decides for the reference drawn under an
        /// instance tag (`None`: not a reference it knows).
        pub fn decision(&self, tag: u32) -> Option<Decision> {
            let (live, tags) = self.live.as_ref()?;
            live.decision(tags.get(&tag)?)
        }

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
                    if let Some(r) = &s.run {
                        self.state = Some(r.clone());
                    }
                    changed |= s.subtitles != self.last.subtitles
                        || s.stage != self.last.stage
                        || (s.now - self.last.now).abs() >= 0.1;
                    self.last = s;
                }
            }
            if let (Some((live, _)), Some(state)) = (self.live.as_mut(), self.state.take()) {
                let b = fo4_core::Budget::new(1 << 32);
                match live.apply(&state, &b) {
                    Ok(n) if n > 0 => self.generation += 1,
                    Ok(_) => {}
                    Err(e) => eprintln!("play: the live world: {e}"),
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
            let boxes = self.bounds.then_some(&self.boxes);
            let hit = match &self.live {
                Some((live, _)) => live.pick(boxes, eye, dir, reach, &|k| {
                    live.decision(k) == Some(Decision::Absent)
                }),
                None => fo4_play::pick::first_of(world, &self.refs, boxes, eye, dir, reach, &|k| {
                    vis.get(k) == Some(&false)
                }),
            };
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
