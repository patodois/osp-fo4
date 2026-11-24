//! The window's play mode (`--play-run`, `spec/play.md`): `quest_run
//! new-game` on its own thread (`fo4_play::link`), fed each frame with the
//! walker's feet, the frame clock and the activation key's target, and the
//! run's second, last stage and spoken lines shown in the title bar. The
//! loop's pieces are tested headless (`fo4_play::link::Frames`, the pick);
//! the window itself has not been opened by anyone (no one can sit at the
//! machine): the play mode's tested path is `play_run`.
//!
//! With `--play-input FILE` the run is driven instead by `play_run`'s
//! walker (`fo4_play::walker::Walker`, built from the same play options
//! through `crate::play_switches`) and that scripted input, on the run's
//! thread, held back by the window's clock ([`Gate`]); the window draws
//! from the walker's eye ([`Seen`]) and loads the cell the walker's walking
//! world was last built about.
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
    pub fn scripted(&self) -> bool {
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
    use fo4_play::{Driver, Next, View};
    use fo4_plugin::FormKey;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::fmt::Write as _;
    use std::rc::Rc;
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::{Arc, Condvar, Mutex};

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
        /// The run's last state the live world applied (applied again to
        /// the live world of a cell loaded later).
        applied: Option<RunState>,
        /// `--play-input`: the scripted run.
        scripted: Option<Scripted>,
    }

    /// The window's side of the scripted run (`--play-input`).
    struct Scripted {
        gate: Arc<Gate>,
        seen: Arc<Mutex<Seen>>,
        /// The scene's switches, for the cells loaded later.
        scene: crate::cli::Scene,
        /// The cell drawn (`None`: the caller's CELL was not given as a key).
        drawn: Option<FormKey>,
        /// A cell being loaded: when, for which second of the run.
        loading: Option<Loading>,
        shots: Option<(String, f64)>,
    }

    /// A cell being loaded: its thread's channel, when it started and the
    /// run's second it was asked at.
    type Loading = (
        Mutex<Receiver<Result<Reloaded, String>>>,
        std::time::Instant,
        f64,
    );

    /// A loaded cell whose upload the window waits for.
    pub struct Loaded {
        pub cell: FormKey,
        pub load_seconds: f64,
        pub started: std::time::Instant,
    }

    /// A cell loaded for the scripted walker.
    pub struct Reloaded {
        pub gpu: crate::scene::GpuScene,
        pub clear: [f32; 4],
        pub cell: FormKey,
        pick: Option<(Vec<Option<FormKey>>, fo4_play::pick::Boxes)>,
        live: Option<LiveLoad>,
        /// Seconds of the load (the loading thread's) and since the
        /// walker reached the cell, and the run's second it was asked at.
        pub load_seconds: f64,
        pub waited: f64,
        pub asked_at: f64,
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
        let sw = &p.switches;
        let (to_run, status, thread, scripted) = match &p.input {
            None => {
                let (to_run, status, thread) = link::spawn(args).map_err(|e| e.to_string())?;
                println!(
                    "play: the run starts at {:?} in {}; E activates within {:?} units",
                    first.pos,
                    fo4_vm::bind::record_key(&first.space),
                    sw.reach
                );
                (to_run, status, thread, None)
            }
            Some(input) => {
                if let Some((dir, _)) = &p.shots {
                    std::fs::create_dir_all(dir).map_err(|e| format!("{dir}: {e}"))?;
                }
                let gate = Arc::new(Gate::default());
                let seen = Arc::new(Mutex::new(Seen::default()));
                let (status_tx, status) = channel();
                // Nothing reads the window's slices: the walker gives them.
                let (to_run, _) = channel();
                let (manifest, input, out) = (s.manifest.clone(), input.clone(), p.out.clone());
                let (sw, g, sn, track) = (sw.clone(), gate.clone(), seen.clone(), t.clone());
                let thread = std::thread::Builder::new()
                    .name("fo4-play scripted run".into())
                    .spawn(move || {
                        let r = scripted_run(
                            &manifest,
                            &input,
                            &out,
                            &sw,
                            args,
                            &track,
                            g,
                            sn.clone(),
                            status_tx,
                        )
                        .map_err(|e| e.to_string());
                        if let Ok(mut s) = sn.lock() {
                            s.done = Some(r.as_ref().map(|_| ()).map_err(Clone::clone));
                        }
                        r
                    })
                    .map_err(|e| e.to_string())?;
                println!(
                    "play: the scripted run starts at {:?} in {}, driven by play_run's walker and {} (--play-input); the run waits for the window's clock",
                    first.pos,
                    fo4_vm::bind::record_key(&first.space),
                    p.input.as_deref().unwrap_or_default()
                );
                let drawn = fo4_scene::timeline::parse_key(&s.cell);
                let scripted = Scripted {
                    gate,
                    seen,
                    scene: s.clone(),
                    drawn,
                    loading: None,
                    shots: p.shots.clone(),
                };
                (to_run, status, thread, Some(scripted))
            }
        };
        Ok(PlayRun {
            frames: Frames::new(first),
            to_run,
            status: Mutex::new(status),
            thread: Some(thread),
            last: Status::default(),
            visibility: Default::default(),
            reach: sw.reach,
            bounds: sw.pick_bounds,
            refs: Vec::new(),
            boxes: Default::default(),
            activations: Vec::new(),
            live_asked: sw.live_world,
            nowait: sw.nowait,
            live: None,
            state: None,
            generation: 0,
            applied: None,
            scripted,
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
            let changed = self.statuses();
            changed.then(|| link::title(&self.last))
        }

        /// Reads the statuses the run sent since the last frame and gives
        /// the live world the run's state; whether the title changed.
        fn statuses(&mut self) -> bool {
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
                if self.scripted.is_some() {
                    self.applied = Some(state);
                }
            }
            changed
        }

        /// Whether the run is driven by a scripted input (`--play-input`).
        pub fn scripted(&self) -> bool {
            self.scripted.is_some()
        }

        /// The shots asked for (`--play-shots DIR --play-shot-every S`).
        pub fn shots(&self) -> Option<&(String, f64)> {
            self.scripted.as_ref()?.shots.as_ref()
        }

        /// The game data's folder (outputs are refused inside it).
        pub fn data_root(&self) -> Option<&str> {
            Some(self.scripted.as_ref()?.scene.data.as_str())
        }

        /// Where the scripted walker stands now.
        pub fn seen(&self) -> Option<Seen> {
            let s = self.scripted.as_ref()?;
            s.seen.lock().ok().map(|s| s.clone())
        }

        /// One frame of the scripted run: the window's clock lets the run
        /// go `dt` seconds further (0 while a cell is loading); the
        /// statuses the run sent are read. The title, when it changed.
        pub fn scripted_frame(&mut self, dt: f64) -> Option<String> {
            if let Some(s) = &self.scripted {
                s.gate.advance(dt);
            }
            self.statuses().then(|| link::title(&self.last))
        }

        /// Whether a cell is being loaded.
        pub fn loading(&self) -> bool {
            self.scripted.as_ref().is_some_and(|s| s.loading.is_some())
        }

        /// The cell the walker's walking world is about when the window
        /// draws another and loads none.
        pub fn cell_wanted(&self, seen: &Seen) -> Option<FormKey> {
            let s = self.scripted.as_ref()?;
            let cell = seen.cell.as_ref()?;
            (s.loading.is_none() && s.drawn.as_ref() != Some(cell)).then(|| cell.clone())
        }

        /// Loads `cell` on its own thread, with the window's scene switches
        /// (its walking world too when the live world was asked for).
        pub fn load(&mut self, cell: FormKey, at: f64) {
            let live = self.live_asked;
            let Some(s) = self.scripted.as_mut() else {
                return;
            };
            let mut scene = s.scene.clone();
            scene.cell = fo4_scene::timeline::key_text(&cell);
            let (tx, rx) = channel();
            let started = std::time::Instant::now();
            println!(
                "play: at {at:.3} s the walker's walking world is about cell {}; the window loads it (the run waits)",
                scene.cell
            );
            let spawned = std::thread::Builder::new()
                .name("fo4-view play cell".into())
                .spawn(move || {
                    let r = crate::load_scene(&scene, true, live)
                        .map(|(gpu, options, mut loaded)| {
                            let mut live = loaded.live.take();
                            if let Some(l) = live.as_mut() {
                                l.world = loaded.walk.take();
                            }
                            Reloaded {
                                gpu,
                                clear: options.clear,
                                cell,
                                pick: loaded.pick.take(),
                                live,
                                load_seconds: started.elapsed().as_secs_f64(),
                                waited: 0.0,
                                asked_at: at,
                            }
                        })
                        .map_err(|e| e.to_string());
                    let _ = tx.send(r);
                });
            match spawned {
                Ok(_) => s.loading = Some((Mutex::new(rx), started, at)),
                Err(e) => eprintln!("play: the cell's loading thread: {e}"),
            }
        }

        /// The cell loaded, when its thread is done: from now on it is the
        /// cell drawn, and the activation and the live world read its load.
        pub fn loaded(&mut self) -> Option<Result<Reloaded, String>> {
            let s = self.scripted.as_mut()?;
            let (rx, started, _) = s.loading.as_ref()?;
            let r = match rx.lock().map(|rx| rx.try_recv()) {
                Ok(Ok(r)) => r,
                Ok(Err(std::sync::mpsc::TryRecvError::Empty)) => return None,
                _ => Err("PlayCellLoad: the loading thread stopped".into()),
            };
            let waited = started.elapsed().as_secs_f64();
            s.loading = None;
            let mut r = match r {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            r.waited = waited;
            s.drawn = Some(r.cell.clone());
            if let Some((refs, boxes)) = r.pick.take() {
                self.refs = refs;
                self.boxes = boxes;
            }
            if let Some(l) = r.live.take() {
                self.live = None;
                match self.follow(l) {
                    Ok(()) => {
                        let state = self.applied.clone();
                        if let (Some((live, _)), Some(state)) = (self.live.as_mut(), state) {
                            let b = fo4_core::Budget::new(1 << 32);
                            match live.apply(&state, &b) {
                                Ok(_) => self.generation += 1,
                                Err(e) => eprintln!("play: the live world: {e}"),
                            }
                        }
                    }
                    Err(e) => eprintln!("play: the live world of the loaded cell: {e}"),
                }
            }
            Some(Ok(r))
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
            if let Some(s) = &self.scripted {
                s.gate.stop();
            }
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

    // ---- The scripted input (`--play-input`) ----

    /// How far the window's clock lets the scripted run go: the run waits
    /// for each slice's second until the window has drawn that far. While
    /// the run is busy (not waiting: a walking world built again, a load
    /// door crossed) the clock does not run ahead of it by more than
    /// [`GATE_BUSY_MAX`], so the window does not draw a place the run has
    /// already left for long.
    #[derive(Default)]
    pub struct Gate {
        state: Mutex<GateState>,
        moved: Condvar,
    }

    #[derive(Default)]
    struct GateState {
        /// The second the run may reach.
        horizon: f64,
        /// The window's seconds drawn while the run was busy, given when it
        /// waits again.
        owed: f64,
        waiting: bool,
        stopped: bool,
    }

    /// The window's seconds a busy run may be owed at most.
    pub const GATE_BUSY_MAX: f64 = 0.25;

    impl Gate {
        /// Waits until the window's clock reaches `until`; `false` when the
        /// window closed.
        fn wait(&self, until: f64) -> bool {
            let Ok(mut g) = self.state.lock() else {
                return false;
            };
            if g.owed > 0.0 {
                g.horizon += std::mem::take(&mut g.owed);
            }
            while g.horizon < until && !g.stopped {
                g.waiting = true;
                g = match self.moved.wait(g) {
                    Ok(g) => g,
                    Err(_) => return false,
                };
            }
            g.waiting = false;
            !g.stopped
        }

        /// The window drew `dt` seconds more: the run may go that much
        /// further when it waits, else (busy) when it next waits, up to
        /// [`GATE_BUSY_MAX`].
        fn advance(&self, dt: f64) {
            if let Ok(mut g) = self.state.lock() {
                if dt.is_finite() && dt > 0.0 {
                    if g.waiting {
                        g.horizon += dt;
                    } else {
                        g.owed = (g.owed + dt).min(GATE_BUSY_MAX);
                    }
                }
            }
            self.moved.notify_all();
        }

        fn stop(&self) {
            if let Ok(mut g) = self.state.lock() {
                g.stopped = true;
            }
            self.moved.notify_all();
        }
    }

    /// Where the scripted walker stands as the window draws it.
    #[derive(Debug, Clone, Default)]
    pub struct Seen {
        /// The second the run may reach (the last slice's end).
        pub t: f64,
        pub feet: Option<[f64; 3]>,
        pub space: Option<FormKey>,
        /// The cell the walker's walking world was last built about (the
        /// start, a grid square left, a load door, a MoveTo, a sit).
        pub cell: Option<FormKey>,
        /// Degrees from +X towards +Y, and above the horizontal.
        pub heading: Option<f64>,
        pub pitch: f64,
        /// The input is over or the run stopped.
        pub finished: bool,
        /// The run's thread is done: its error, if any.
        pub done: Option<Result<(), String>>,
    }

    /// The scripted walker as the run's driver, held back by the window's
    /// clock; what the window shows is sent through the keyboard's link
    /// ([`fo4_play::link::Link`]) unchanged.
    struct Paced {
        walker: fo4_play::walker::Walker,
        log: Rc<RefCell<fo4_play::walker::Log>>,
        gate: Arc<Gate>,
        seen: Arc<Mutex<Seen>>,
        link: link::Link,
        feed: Sender<ToRun>,
        /// The log's grid rebuilds, crossings and looks already read.
        read: (usize, usize, usize),
    }

    impl Paced {
        /// Where the walker was settled at the start, before the first
        /// slice waits for the window's clock (which starts once the
        /// walker stands somewhere).
        fn publish_start(&self) {
            let log = self.log.borrow();
            let Ok(mut s) = self.seen.lock() else {
                return;
            };
            if s.feet.is_none() {
                if let Some((p, feet, _)) = &log.start {
                    s.feet = Some(*feet);
                    s.space = Some(p.space.clone());
                }
            }
        }

        fn publish(&mut self, n: &Next) {
            let log = self.log.borrow();
            let Ok(mut s) = self.seen.lock() else {
                return;
            };
            // The heading: the slice's horizontal move, else a look command
            // given at or after its last point (`face`, `turn`: commands
            // due at a slice's end come after its move), else as it was.
            let look = (log.looks.len() > self.read.2)
                .then(|| log.looks.last())
                .flatten()
                .copied();
            if let Some((_, _, pitch)) = look {
                s.pitch = pitch;
            }
            let mut moved = None;
            match n {
                Next::Slice(slice) => {
                    if let Some(p) = slice.points.last() {
                        let looked_after = look.is_some_and(|l| l.0 >= p.t);
                        if let (Some(f), true, false) =
                            (s.feet, s.space.as_ref() == Some(&p.space), looked_after)
                        {
                            moved = Some([p.pos[0] - f[0], p.pos[1] - f[1]]);
                        }
                        s.feet = Some(p.pos);
                        s.space = Some(p.space.clone());
                    }
                    s.t = slice.until;
                }
                Next::Finish | Next::Stop => s.finished = true,
            }
            s.heading = heading(s.heading, moved, look.map(|l| l.1));
            let grid = log
                .grid
                .last()
                .filter(|_| log.grid.len() > self.read.0)
                .map(|g| (g.0, g.1.clone()));
            let door = log
                .doors
                .last()
                .filter(|_| log.doors.len() > self.read.1)
                .map(|d| (d.t, d.cell.clone()));
            let newest = match (grid, door) {
                (Some(g), Some(d)) => Some(if d.0 >= g.0 { d.1 } else { g.1 }),
                (g, d) => g.or(d).map(|x| x.1),
            };
            if let Some(c) = newest {
                s.cell = Some(c);
            }
            self.read = (log.grid.len(), log.doors.len(), log.looks.len());
        }
    }

    /// A horizontal move shorter than this (units) leaves the heading.
    const HEADING_MOVE: f64 = 0.5;

    /// The camera's heading (degrees from +X towards +Y) after a slice:
    /// along the slice's horizontal move when it moved, else the heading a
    /// look command of the slice set, else the one before.
    fn heading(before: Option<f64>, moved: Option<[f64; 2]>, look: Option<f64>) -> Option<f64> {
        match moved {
            Some(d) if d[0].hypot(d[1]) > HEADING_MOVE => Some(d[1].atan2(d[0]).to_degrees()),
            _ => look.or(before),
        }
    }

    impl Driver for Paced {
        fn next(&mut self, run: &View<'_, '_>) -> Next {
            let mut n = self.walker.next(run);
            self.publish_start();
            if let Next::Slice(s) = &n {
                if !self.gate.wait(s.until) {
                    n = Next::Stop;
                }
            }
            self.publish(&n);
            let _ = self.feed.send(match n {
                Next::Slice(s) => ToRun::Slice(s),
                Next::Finish => ToRun::Finish,
                Next::Stop => ToRun::Stop,
            });
            self.link.next(run)
        }
    }

    /// The run's track as `play_run` writes it (`play-track.txt`).
    fn track_text(points: &[fo4_play::Point]) -> String {
        let mut s = String::from("# the run's track in the play mode: SECONDS SPACE X Y Z\n");
        for p in points {
            let _ = writeln!(
                s,
                "{} {} {} {} {}",
                p.t,
                fo4_vm::bind::record_key(&p.space),
                p.pos[0],
                p.pos[1],
                p.pos[2]
            );
        }
        s
    }

    /// What the walker did, in `play_run`'s words (the lines the window's
    /// run is compared by).
    fn walker_report(l: &fo4_play::walker::Log, report: &mut String) {
        if let Some((p, feet, ticks)) = &l.start {
            let _ = writeln!(
                report,
                "start: track point {:?} at {} s; walker settled in {ticks} ticks at {:?}",
                p.pos, p.t, feet
            );
        }
        let _ = writeln!(
            report,
            "walker: {} ticks, {} slices ({} standing), {} points, {} blocked, {} stepped, {} airborne",
            l.ticks, l.slices, l.standing_slices, l.points, l.blocked, l.stepped, l.airborne
        );
        if let Some(s) = &l.live {
            let _ = writeln!(
                report,
                "live world: {} applies of the run's state, {} references entered (or came back), {} left, {} moved",
                s.applies, s.entered, s.left, s.moved
            );
        }
        for (t, cell, feet, secs) in &l.grid {
            let _ = writeln!(
                report,
                "grid: at {t} s the walking world built again about cell {} (feet {feet:?}) in {secs:.2} s",
                fo4_vm::bind::record_key(cell)
            );
        }
        for d in &l.doors {
            let _ = writeln!(
                report,
                "{} at {} s: {} to {} in {}: cell {}, position {:?}, feet settled at {:?}; walking world built in {:.2} s",
                d.rule,
                d.t,
                fo4_vm::bind::record_key(&d.door),
                fo4_vm::bind::record_key(&d.to.to),
                fo4_vm::bind::record_key(&d.to.space),
                fo4_vm::bind::record_key(&d.cell),
                d.to.pos,
                d.feet,
                d.seconds,
            );
        }
        for (t, why) in &l.player_moves {
            let _ = writeln!(report, "player place at {t} s: {why}");
        }
        for (t, x) in &l.elevators {
            let _ = writeln!(report, "elevator at {t} s: {x}");
        }
        for r in &l.rules {
            let _ = writeln!(report, "rule relied on\t{r}");
        }
        for (t, eye, dir, what) in &l.activations {
            let what = match what {
                Ok((k, d)) => format!("{} at {d:.1} units", fo4_vm::bind::record_key(k)),
                Err(why) => why.clone(),
            };
            let _ = writeln!(
                report,
                "activate at {t} s from {eye:?} along {dir:?}: {what}"
            );
        }
    }

    type E = Box<dyn std::error::Error>;

    /// The scripted run on its own thread: the walking world built about
    /// the track's first point and the walker driven by the input file, as
    /// `play_run` builds them; `play.txt` and `play-track.txt` written to
    /// the run's output folder at its end.
    #[allow(clippy::too_many_arguments)]
    fn scripted_run(
        manifest: &str,
        input: &str,
        out: &str,
        switches: &crate::play_switches::Switches,
        args: Vec<String>,
        track: &fo4_vm::track::Track,
        gate: Arc<Gate>,
        seen: Arc<Mutex<Seen>>,
        status: Sender<Status>,
    ) -> Result<BTreeMap<String, u64>, E> {
        use fo4_core::{Budget, Manifest};
        let started = std::time::Instant::now();
        let script = fo4_play::input::Script::parse(&std::fs::read_to_string(input)?)?;
        let m = Manifest::load(std::path::Path::new(manifest))?;
        let store = fo4_plugin::typed::Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
        let vfs = fo4_vfs::Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
        let first = track.points.first().ok_or("an empty track")?;
        let b = Budget::new(m.limits.max_memory);
        let mut resolved = track.clone();
        resolved.resolve(&store)?;
        let space = resolved.points[0].space.clone();
        let cell = match store.signature(&first.space) {
            Some(s) if s == *b"CELL" => first.space.clone(),
            _ => fo4_play::world::start_cell(&store, &space, first.pos, &b)?,
        };
        let built = fo4_play::world::build(&m, &store, &vfs, &cell, switches.world_options())?;
        if let Ok(mut s) = seen.lock() {
            s.cell = Some(built.cell.clone());
        }
        let mut report = String::new();
        let walker = switches.walker(built, script, &mut report)?;
        drop((store, vfs));
        let log = walker.log.clone();
        let (feed, from_walker) = channel();
        let paced = Paced {
            walker,
            log: log.clone(),
            gate,
            seen,
            link: link::Link::new(from_walker, status),
            feed,
            read: (0, 0, 0),
        };
        let prepared = started.elapsed().as_secs_f64();
        let (_, counts) =
            fo4_play::new_game_with(args, Some(Box::new(paced)), switches.run_options())?;
        let _ = writeln!(
            report,
            "seconds: walk world {prepared:.2}, run {:.2} (paced by the window's clock)",
            started.elapsed().as_secs_f64() - prepared
        );
        for (k, v) in &counts {
            let _ = writeln!(report, "{k}\t{v}");
        }
        let l = log.borrow();
        walker_report(&l, &mut report);
        let mut points: Vec<fo4_play::Point> = track
            .points
            .iter()
            .filter(|p| p.t <= 0.0)
            .cloned()
            .collect();
        points.extend(l.track.iter().cloned());
        let dir = std::path::Path::new(out);
        std::fs::write(dir.join("play-track.txt"), track_text(&points))?;
        std::fs::write(dir.join("play.txt"), report)?;
        Ok(counts)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_gate_holds_the_run_until_the_window_draws_that_far() {
            let gate = Arc::new(Gate::default());
            // Frames drawn while the run is busy (not waiting): owed, at
            // most GATE_BUSY_MAX; no time from a frame of no finite or a
            // negative length.
            for _ in 0..4 {
                gate.advance(0.25);
            }
            gate.advance(f64::NAN);
            gate.advance(-1.0);
            let (first, waited) = channel();
            let g = gate.clone();
            let run = std::thread::spawn(move || {
                // The owed 0.25 s let the first slice go at once.
                let _ = first.send(g.wait(GATE_BUSY_MAX));
                // The next waits for the window's frames.
                g.wait(1.0)
            });
            assert!(waited.recv().unwrap());
            // Frames of 0.25 s until the run is through (it waits between).
            let mut n = 0;
            while !run.is_finished() && n < 10_000 {
                gate.advance(0.25);
                n += 1;
                std::thread::yield_now();
            }
            assert!(run.join().unwrap());
            let h = gate.state.lock().unwrap().horizon;
            assert!(h >= 1.0, "{h}");
            // The window closed: a run waiting is let go, refused.
            gate.stop();
            assert!(!gate.wait(h + 1.0));
        }

        #[test]
        fn the_camera_looks_along_the_move_else_where_the_input_faced() {
            assert_eq!(heading(None, None, None), None);
            assert_eq!(heading(Some(5.0), None, None), Some(5.0));
            // A face command while standing.
            assert_eq!(heading(Some(5.0), Some([0.1, 0.0]), Some(10.0)), Some(10.0));
            // A move wins over the command's heading.
            assert_eq!(heading(Some(5.0), Some([0.0, 2.0]), Some(10.0)), Some(90.0));
            assert_eq!(heading(None, Some([-3.0, 0.0]), None), Some(180.0));
        }

        #[test]
        fn the_play_track_is_play_runs() {
            let p = fo4_play::Point {
                t: 1.5,
                space: FormKey {
                    plugin: "a.esm".into(),
                    object_id: 0x3c,
                },
                pos: [1.0, -2.0, 3.25],
                facing: None,
            };
            assert_eq!(
                track_text(&[p]),
                "# the run's track in the play mode: SECONDS SPACE X Y Z\n1.5 a.esm:0000003C 1 -2 3.25\n"
            );
        }
    }
}
