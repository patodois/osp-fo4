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
    use std::sync::atomic::{AtomicBool, Ordering};
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
        /// `--play-collision-child-flag-agrees`: the window's live world
        /// decides as the run's walker does.
        child_flag_agrees: bool,
        /// The live world and the drawn references by instance tag.
        pub live: Option<(Live, BTreeMap<u32, FormKey>)>,
        /// The run's last state, and how many times it was applied.
        state: Option<RunState>,
        pub generation: u64,
        /// The run's last state the live world applied (applied again to
        /// the live world of a cell loaded later).
        applied: Option<RunState>,
        /// `--play-input` or `--play-keys`: the scripted run.
        scripted: Option<Scripted>,
        /// `--play-keys-from`: the synthetic person's events and the
        /// walking speed it steers at (taken by the window).
        pub synthetic: Option<(Vec<crate::person::Timed>, f64)>,
        /// The viewport's crosshair and lines (`--play-hud`, on with
        /// `--play-keys`; `crate::hud`).
        pub hud: bool,
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
        /// `--play-shots-between FROM TO EVERY`.
        shots_between: Vec<[f64; 3]>,
        /// `--timeline-cast`: the cast's worker and what it built.
        cast: Option<CastWork>,
        /// `--play-keys`: a person's keys drive the walker.
        keys: bool,
        /// The play data thread ([`data_worker`]), started with the run:
        /// the cell loads and the cast's builds.
        data: Sender<DataAsk>,
        /// Set when a cell load is asked of the play data thread, cleared
        /// when it starts it: a cast being built gives way.
        cell_asked: Arc<AtomicBool>,
        /// The window's first cell, until [`PlayRun::prepare`] asks the
        /// play data thread to read it again.
        prepare: Option<(Box<crate::cli::Scene>, bool)>,
    }

    /// A cell load asked of the play data thread: the scene's switches for
    /// the cell, the live world, where the result goes and when it was
    /// asked.
    struct CellAsk {
        scene: Box<crate::cli::Scene>,
        cell: FormKey,
        live: bool,
        at: f64,
        started: std::time::Instant,
        reply: Sender<Result<Reloaded, String>>,
        /// The cell drawn when it was asked (`None`: not as a key).
        from: Option<FormKey>,
        /// `--timeline-cast`: the new cell's cast, built right after the
        /// cell on the play data thread (viewer policy).
        cast: Option<CastAsk>,
    }

    /// The window's cast (`--play-run ... --timeline-cast`): the play data
    /// thread builds the run's actors of a cell at a second of the run's
    /// timeline ([`crate::timeline::cast_scene`]).
    struct CastWork {
        to: Sender<DataAsk>,
        from: Mutex<Receiver<CastDone>>,
        /// The request on its way: its cell and signature.
        asked: Option<(FormKey, CastSig)>,
        /// The signature of the cast drawn (or being uploaded) and its cell.
        built: Option<(FormKey, CastSig)>,
        /// The timeline's relevant event seconds, of the snapshot `gen`.
        times: (u64, Vec<f64>),
        /// Builds drawn, for the report.
        builds: u64,
    }

    impl CastWork {
        /// The signature of the cast at `second` of timeline `t` (snapshot
        /// `gen`): its cast events up to that second.
        fn signature(
            &mut self,
            t: &fo4_scene::timeline::Timeline,
            gen: u64,
            second: f64,
        ) -> CastSig {
            if self.times.0 != gen {
                let mut times: Vec<f64> = t
                    .events
                    .iter()
                    .filter(|e| cast_event(&e.what))
                    .map(|e| e.t)
                    .collect();
                times.sort_by(f64::total_cmp);
                self.times = (gen, times);
            }
            self.times.1.partition_point(|x| *x <= second)
        }
    }

    /// What decides the cast of a cell at a second: the timeline's
    /// actor, visibility, death and sit events up to that second (their
    /// count).
    type CastSig = usize;

    struct CastAsk {
        cell: FormKey,
        timeline: Arc<fo4_scene::timeline::Timeline>,
        second: f64,
        sig: CastSig,
    }

    /// The cast built for a cell and signature; `Ok(None)`: set aside
    /// unbuilt because a cell load was asked.
    type CastDone = (
        FormKey,
        CastSig,
        Result<Option<crate::timeline::CastScene>, String>,
    );

    /// The timeline's events a cast depends on: an actor's place, a
    /// visibility change, a death, a sit or a get-up.
    fn cast_event(w: &fo4_scene::timeline::What) -> bool {
        use fo4_scene::timeline::What;
        match w {
            What::Actor { .. } | What::Visibility { .. } => true,
            What::Note(t) => fo4_scene::timeline::body_note(t).is_some(),
            _ => false,
        }
    }

    /// What the play data thread is asked: a cast, a cell load, or the
    /// window's first cell read again for the assets it leaves the first
    /// change of cell (`Prepare`: the scene's switches and the live world;
    /// what it draws is dropped).
    enum DataAsk {
        Cast(CastAsk),
        Cell(CellAsk),
        Prepare(Box<crate::cli::Scene>, bool),
    }

    /// Whether two cells lie in one worldspace (both exterior cells of it),
    /// so that what is placed in one stands where it stands in the other.
    fn same_world(store: &fo4_plugin::typed::Store, a: &FormKey, b: &FormKey) -> bool {
        let norm = fo4_scene::timeline::norm;
        match (store.world(a), store.world(b)) {
            (Some(x), Some(y)) => norm(&x) == norm(&y),
            _ => false,
        }
    }

    /// What the play data thread does next of what it was asked (`queue`,
    /// in the order asked) and the first cell's read kept aside
    /// (`prepare`): a cell load first; else the newest cast, the older ones
    /// dropped; else the first cell read again ([`DataAsk::Prepare`] is
    /// taken out of the queue into `prepare` as it comes).
    fn next_ask(
        queue: &mut std::collections::VecDeque<DataAsk>,
        prepare: &mut Option<(Box<crate::cli::Scene>, bool)>,
    ) -> Option<DataAsk> {
        if let Some(i) = queue.iter().position(|a| matches!(a, DataAsk::Prepare(..))) {
            if let Some(DataAsk::Prepare(s, live)) = queue.remove(i) {
                *prepare = Some((s, live));
            }
        }
        match queue.iter().position(|a| matches!(a, DataAsk::Cell(_))) {
            Some(i) => queue.remove(i),
            None => match queue.pop_back() {
                Some(last) => {
                    queue.clear();
                    Some(last)
                }
                None => prepare.take().map(|(s, live)| DataAsk::Prepare(s, live)),
            },
        }
    }

    /// The window's play data thread (viewer policy): it opens the game
    /// data once as the run starts (the plugins and the archives,
    /// `crate::load::Sources`) and keeps it for the cast's builds
    /// ([`crate::timeline::cast_scene`]) and the cell loads
    /// ([`crate::load_scene_with`]), so a change of cell does not open them
    /// again and the window keeps one set of them open for both. A cell
    /// load asked is done first (the window waits for it): a cast being
    /// built when one is asked (`cell_asked`) is set aside between two of
    /// its steps and the cell loaded (the window asks the new cell's cast
    /// once it is drawn); of the cast's requests only the newest counts.
    /// What it loads is what a load that opens the data gives.
    fn data_worker(
        scene: crate::cli::Scene,
        cast: Option<(crate::timeline::Args, Sender<CastDone>)>,
        asks: Receiver<DataAsk>,
        cell_asked: Arc<AtomicBool>,
    ) {
        let opened = std::time::Instant::now();
        let sources = crate::load::open(&scene.manifest, &scene.data).map_err(|e| e.to_string());
        match &sources {
            Ok(_) => println!(
                "play: the play data thread's game data opened in {:.2} s, kept for the cast and the cell loads",
                opened.elapsed().as_secs_f64()
            ),
            Err(e) => eprintln!("play: the play data thread's sources: {e}"),
        }
        // What one cell load leaves for the next (`load::KeptAssets`).
        let mut kept = crate::load::KeptAssets::default();
        // The window's first cell, read again here for its assets
        // ([`DataAsk::Prepare`]), once nothing else is asked.
        let mut prepare: Option<(Box<crate::cli::Scene>, bool)> = None;
        let mut queue: std::collections::VecDeque<DataAsk> = std::collections::VecDeque::new();
        loop {
            if queue.is_empty() && prepare.is_none() {
                match asks.recv() {
                    Ok(a) => queue.push_back(a),
                    Err(_) => return,
                }
            }
            while let Ok(a) = asks.try_recv() {
                queue.push_back(a);
            }
            match next_ask(&mut queue, &mut prepare) {
                Some(DataAsk::Cell(mut a)) => {
                    cell_asked.store(false, Ordering::SeqCst);
                    let keep_cast = match (&sources, &a.from) {
                        (Ok(src), Some(from)) => same_world(&src.store, from, &a.cell),
                        _ => false,
                    };
                    let cast_after = a.cast.take();
                    let r = match &sources {
                        Ok(src) => crate::load_scene_with(
                            &a.scene,
                            true,
                            a.live,
                            Some(src),
                            Some(&mut kept),
                        )
                        .map(|(gpu, options, mut loaded)| {
                            let mut live = loaded.live.take();
                            if let Some(l) = live.as_mut() {
                                l.world = loaded.walk.take();
                            }
                            Reloaded {
                                gpu,
                                clear: options.clear,
                                cell: a.cell,
                                pick: loaded.pick.take(),
                                live,
                                load_seconds: a.started.elapsed().as_secs_f64(),
                                waited: 0.0,
                                asked_at: a.at,
                                keep_cast,
                            }
                        })
                        .map_err(|e| e.to_string()),
                        Err(e) => Err(format!("PlayCellSources: {e}")),
                    };
                    let loaded = r.is_ok();
                    let _ = a.reply.send(r);
                    // The new cell's cast next, unless a cell is asked
                    // before it starts.
                    if let (true, Some(c)) = (loaded, cast_after) {
                        queue.push_front(DataAsk::Cast(c));
                    }
                }
                Some(DataAsk::Prepare(first, live)) => {
                    let Ok(src) = &sources else {
                        continue;
                    };
                    let started = std::time::Instant::now();
                    match crate::load_scene_with(&first, true, live, Some(src), Some(&mut kept)) {
                        Ok(_) => println!(
                            "play: the window's first cell read again on the play data thread in {:.2} s, its assets kept for the first change of cell (what it drew dropped)",
                            started.elapsed().as_secs_f64()
                        ),
                        Err(e) => eprintln!("play: the first cell read again: {e}"),
                    }
                }
                Some(DataAsk::Cast(ask)) => {
                    let (Some((args, done)), Ok(src)) = (&cast, &sources) else {
                        continue;
                    };
                    let started = std::time::Instant::now();
                    let stop = || cell_asked.load(Ordering::SeqCst);
                    let r = if stop() {
                        Ok(None)
                    } else {
                        crate::timeline::cast_scene(
                            src,
                            &scene,
                            args,
                            &ask.cell,
                            &ask.timeline,
                            ask.second,
                            &stop,
                            Some(&mut kept),
                        )
                        .map_err(|e| e.to_string())
                    };
                    if matches!(r, Ok(None)) {
                        println!(
                            "play: cast of {} at {:.3} s of the run set aside after {:.2} s: a cell load was asked",
                            fo4_scene::timeline::key_text(&ask.cell),
                            ask.second,
                            started.elapsed().as_secs_f64()
                        );
                    }
                    if done.send((ask.cell, ask.sig, r)).is_err() {
                        return;
                    }
                }
                None => {}
            }
        }
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
        /// The cell drawn before lies in the same worldspace: the cast
        /// drawn stays until the new cell's is uploaded (viewer policy).
        pub keep_cast: bool,
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
        // --play-activate-opens: the caller's timed opens of those
        // references are left to the walker's activations.
        let args = p.switches.run_args(args);
        let sw = &p.switches;
        let synthetic = match &p.input {
            Some(crate::cli::Input::Keys(crate::cli::Keys {
                synthetic: Some(file),
                ..
            })) => {
                let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
                let events = crate::person::parse_synthetic(&text)?;
                let speed = sw
                    .walk_speed
                    .unwrap_or(fo4_walk::Params::PROVISIONAL.walk_speed);
                println!(
                    "play: a synthetic person (--play-keys-from {file}, test and measurement only): {} events injected as key, button and mouse input",
                    events.len()
                );
                Some((events, speed))
            }
            _ => None,
        };
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
                let keys = matches!(input, crate::cli::Input::Keys(_));
                if let Some((dir, _)) = &p.shots {
                    std::fs::create_dir_all(dir).map_err(|e| format!("{dir}: {e}"))?;
                }
                let gate = Arc::new(Gate::default());
                let seen = Arc::new(Mutex::new(Seen::default()));
                let (status_tx, status) = channel();
                // Nothing reads the window's slices: the walker gives them.
                let (to_run, _) = channel();
                let (manifest, run_input, out) = (s.manifest.clone(), input.clone(), p.out.clone());
                let data = s.data.clone();
                let (sw, g, sn, track) = (sw.clone(), gate.clone(), seen.clone(), t.clone());
                let with_timeline = s.play_cast.is_some();
                let thread = std::thread::Builder::new()
                    .name("fo4-play scripted run".into())
                    .spawn(move || {
                        let r = scripted_run(
                            &manifest,
                            &run_input,
                            &data,
                            &out,
                            &sw,
                            args,
                            &track,
                            g,
                            sn.clone(),
                            status_tx,
                            with_timeline,
                        )
                        .map_err(|e| e.to_string());
                        if let Ok(mut s) = sn.lock() {
                            s.done = Some(r.as_ref().map(|_| ()).map_err(Clone::clone));
                        }
                        r
                    })
                    .map_err(|e| e.to_string())?;
                match input.file() {
                    Some(file) => println!(
                        "play: the scripted run starts at {:?} in {}, driven by play_run's walker and {file} (--play-input); the run waits for the window's clock",
                        first.pos,
                        fo4_vm::bind::record_key(&first.space),
                    ),
                    None => println!(
                        "play: the run starts at {:?} in {}, driven by play_run's walker and the keyboard and mouse (--play-keys): W/S/A/D move, Shift runs, the left mouse button held turns and looks, E activates, P pauses, Escape ends the input; the run waits for the window's clock",
                        first.pos,
                        fo4_vm::bind::record_key(&first.space),
                    ),
                }
                let drawn = fo4_scene::timeline::parse_key(&s.cell);
                let (data, asks) = channel();
                let (done, from) = channel();
                let (scene, cast_args) = (s.clone(), s.play_cast.clone());
                let cell_asked = Arc::new(AtomicBool::new(false));
                let asked = cell_asked.clone();
                std::thread::Builder::new()
                    .name("fo4-view play data".into())
                    .spawn(move || data_worker(scene, cast_args.map(|a| (a, done)), asks, asked))
                    .map_err(|e| format!("play: the play data thread: {e}"))?;
                // The window loads its first cell itself; once it draws, the
                // play data thread reads it again ([`PlayRun::prepare`]).
                let prepare = Some((Box::new(s.clone()), p.switches.live_world));
                let cast = match &s.play_cast {
                    Some(a) => {
                        println!(
                            "play: the run's actors drawn as their characters from the run's timeline (--timeline-cast{}), built again on their own thread when the run changes an actor's place or presence",
                            if a.pose.any() { ", start clip" } else { "" }
                        );
                        Some(CastWork {
                            to: data.clone(),
                            from: Mutex::new(from),
                            asked: None,
                            built: None,
                            times: (u64::MAX, Vec::new()),
                            builds: 0,
                        })
                    }
                    None => None,
                };
                let scripted = Scripted {
                    gate,
                    seen,
                    scene: s.clone(),
                    drawn,
                    loading: None,
                    shots: p.shots.clone(),
                    shots_between: p.shots_between.clone(),
                    cast,
                    keys,
                    data,
                    cell_asked,
                    prepare,
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
            child_flag_agrees: sw.child_flag_agrees,
            live: None,
            state: None,
            generation: 0,
            applied: None,
            scripted,
            synthetic,
            hud: p.hud,
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
            .map_err(|e| e.to_string())?
            .with_child_flag_agrees(self.child_flag_agrees);
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

        /// Whether the run is driven by `play_run`'s walker (`--play-input`
        /// or `--play-keys`).
        pub fn scripted(&self) -> bool {
            self.scripted.is_some()
        }

        /// Whether a person's keys drive the walker (`--play-keys`).
        pub fn keys(&self) -> bool {
            self.scripted.as_ref().is_some_and(|s| s.keys)
        }

        /// One frame of the person's mode: the window's clock lets the run
        /// go `dt` seconds further (0 while paused or a cell is loading),
        /// the person's commands of the frame stamped at the play clock's
        /// second they reach the walker at (`Gate::submit`). Returns the
        /// title, when it changed, and the play clock (that second: the
        /// walker's clock with what a busy run is owed, which stops
        /// [`GATE_BUSY_MAX`] ahead of a busy run while the window's frames
        /// go on).
        pub fn person_frame(
            &mut self,
            dt: f64,
            wall: f64,
            commands: Vec<fo4_play::input::Command>,
        ) -> (Option<String>, f64) {
            let mut clock = 0.0;
            if let Some(s) = &self.scripted {
                let (events, at) = s.gate.submit(dt, wall, commands);
                clock = at;
                for e in events {
                    if !matches!(
                        e.command,
                        fo4_play::input::Command::Face(_) | fo4_play::input::Command::Pitch(_)
                    ) {
                        println!("play: person: {e}");
                    }
                }
            }
            (self.statuses().then(|| link::title(&self.last)), clock)
        }

        /// The shots asked for (`--play-shots DIR --play-shot-every S`).
        pub fn shots(&self) -> Option<&(String, f64)> {
            self.scripted.as_ref()?.shots.as_ref()
        }

        /// The second of the frame after one written at `after`: the next
        /// multiple of the shot period, or sooner the next second of a
        /// denser stretch (`--play-shots-between`).
        pub fn next_shot(&self, after: f64) -> Option<f64> {
            let s = self.scripted.as_ref()?;
            let (_, every) = s.shots.as_ref()?;
            Some(next_shot(after, *every, &s.shots_between))
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

        /// Of those, the texts the data settles (the viewport's subtitle).
        pub fn spoken_texts(&self) -> &[String] {
            &self.last.texts
        }

        /// The objective the run shows (its text), at the last status.
        pub fn objective(&self) -> Option<&str> {
            self.last.objective.as_deref()
        }

        /// The title with the play clock's second (the walker's clock, what
        /// the window draws), not the run's last event's.
        pub fn title_at(&self, clock: f64) -> String {
            link::title_at(&self.last, clock)
        }

        /// Whether the run's actors are drawn (`--timeline-cast`).
        pub fn casts(&self) -> bool {
            self.scripted.as_ref().is_some_and(|s| s.cast.is_some())
        }

        /// Whether a cell is being loaded.
        pub fn loading(&self) -> bool {
            self.scripted.as_ref().is_some_and(|s| s.loading.is_some())
        }

        /// The cell the walker's walking world is about (or is being built
        /// about: the window loads it while the walker builds) when the
        /// window draws another and loads none.
        pub fn cell_wanted(&self, seen: &Seen) -> Option<FormKey> {
            let s = self.scripted.as_ref()?;
            let cell = seen.next_cell.as_ref().or(seen.cell.as_ref())?;
            (s.loading.is_none() && s.drawn.as_ref() != Some(cell)).then(|| cell.clone())
        }

        /// Loads `cell` on its own thread, with the window's scene switches
        /// (its walking world too when the live world was asked for); with
        /// `--timeline-cast` the play data thread builds the new cell's
        /// cast at the run's second `at` right after (viewer policy).
        pub fn load(&mut self, cell: FormKey, at: f64, seen: &Seen) {
            let live = self.live_asked;
            let Some(s) = self.scripted.as_mut() else {
                return;
            };
            let cast = match (s.cast.as_mut(), &seen.timeline) {
                (Some(c), Some(t)) => {
                    let sig = c.signature(t, seen.timeline_gen, at);
                    c.asked = Some((cell.clone(), sig));
                    Some(CastAsk {
                        cell: cell.clone(),
                        timeline: t.clone(),
                        second: at,
                        sig,
                    })
                }
                _ => None,
            };
            let mut scene = s.scene.clone();
            scene.cell = fo4_scene::timeline::key_text(&cell);
            // `--exposure-eye` is measured from the walker's eye at the
            // load (once per cell, not per frame).
            if let Some(view) = s.seen.lock().ok().and_then(|x| eye_look(&x)) {
                (scene.eye, scene.look) = view;
            }
            let (tx, rx) = channel();
            let started = std::time::Instant::now();
            println!(
                "play: at {at:.3} s the walker's walking world is about cell {}; the window loads it (the run waits)",
                scene.cell
            );
            let ask = CellAsk {
                scene: Box::new(scene),
                cell,
                live,
                at,
                started,
                reply: tx,
                from: s.drawn.clone(),
                cast,
            };
            s.cell_asked.store(true, Ordering::SeqCst);
            match s.data.send(DataAsk::Cell(ask)) {
                Ok(()) => s.loading = Some((Mutex::new(rx), started, at)),
                Err(_) => eprintln!("play: the play data thread is gone"),
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
            // The new cell's cast is built anew.
            if let Some(c) = s.cast.as_mut() {
                c.built = None;
            }
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

        /// Asks the play data thread, once, to read the window's first cell
        /// again when it has nothing else to do ([`DataAsk::Prepare`]): the
        /// first change of cell then starts from its assets instead of an
        /// empty cache. Called each frame after the cast is asked, so the
        /// first cast is built before it.
        pub fn prepare(&mut self) {
            let Some(s) = self.scripted.as_mut() else {
                return;
            };
            if let Some((scene, live)) = s.prepare.take() {
                if s.data.send(DataAsk::Prepare(scene, live)).is_err() {
                    eprintln!("play: the play data thread is gone");
                }
            }
        }

        /// `--timeline-cast`: asks the cast's thread for the run's actors of
        /// the drawn cell at the run's second when what decides them
        /// changed since the cast drawn (or asked for), none being built.
        pub fn cast_ask(&mut self, seen: &Seen) {
            let Some(s) = self.scripted.as_mut() else {
                return;
            };
            let (Some(c), Some(cell), Some(t)) = (s.cast.as_mut(), s.drawn.clone(), &seen.timeline)
            else {
                return;
            };
            // One at a time, and none while a cell loads: the new cell's
            // cast is asked once it is drawn.
            if c.asked.is_some() || s.loading.is_some() {
                return;
            }
            let sig = c.signature(t, seen.timeline_gen, seen.t);
            if c.built.as_ref() == Some(&(cell.clone(), sig)) {
                return;
            }
            let ask = CastAsk {
                cell: cell.clone(),
                timeline: t.clone(),
                second: seen.t,
                sig,
            };
            if c.to.send(DataAsk::Cast(ask)).is_ok() {
                c.asked = Some((cell, sig));
            }
        }

        /// The cast the thread built, when it is for the cell drawn: the
        /// window draws it in place of the one before.
        pub fn cast_done(&mut self) -> Option<crate::timeline::CastScene> {
            let s = self.scripted.as_mut()?;
            let drawn = s.drawn.clone();
            let c = s.cast.as_mut()?;
            let (cell, sig, r) = c.from.lock().ok()?.try_recv().ok()?;
            // A cast asked before the one asked with a cell load (set
            // aside, or built for the cell before) leaves that one asked.
            if c.asked.as_ref() == Some(&(cell.clone(), sig)) {
                c.asked = None;
            }
            if drawn.as_ref() != Some(&cell) {
                return None;
            }
            match r {
                Ok(Some(x)) => {
                    c.built = Some((cell, sig));
                    c.builds += 1;
                    Some(x)
                }
                // Set aside for a cell load: asked again when due.
                Ok(None) => None,
                Err(e) => {
                    eprintln!("play: the cast: {e}");
                    // Not asked again until something changes.
                    c.built = Some((cell, sig));
                    None
                }
            }
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
                if let Some(c) = &s.cast {
                    let cost = s.seen.lock().map(|x| x.timeline_cost).unwrap_or_default();
                    println!(
                        "play: cast: {} casts drawn; the run's timeline read {} times in {:.2} s on the run's thread",
                        c.builds, cost.0, cost.1
                    );
                }
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
        /// The person's commands at their seconds, not yet taken by the
        /// walker (`--play-keys`), in the order of their seconds.
        queued: Vec<fo4_play::input::Event>,
        /// The play clock at the last frame that gave the person's
        /// commands a second, and the second no later command goes before
        /// (after commands made while the clock stood).
        stamped: f64,
        floor: f64,
        /// Commands made while the play clock stood (a walking world built
        /// again, a cell loading), with the window's wall clock when each
        /// was made: given when the clock runs again ([`Gate::submit`]).
        deferred: Vec<(f64, fo4_play::input::Command)>,
    }

    impl GateState {
        /// The window drew `dt` seconds more: the horizon moves while the
        /// run waits, else the run is owed them, up to [`GATE_BUSY_MAX`].
        fn add(&mut self, dt: f64) {
            if dt.is_finite() && dt > 0.0 {
                if self.waiting {
                    self.horizon += dt;
                } else {
                    self.owed = (self.owed + dt).min(GATE_BUSY_MAX);
                }
            }
        }
    }

    /// The window's seconds a busy run may be owed at most.
    pub const GATE_BUSY_MAX: f64 = 0.25;

    /// Commands made while the play clock stood are given as it runs
    /// again, spread as they were made, each gap between two of them kept
    /// up to this many seconds of the play clock (a viewer policy: a key
    /// tapped during a load still moves the walker for as long as it was
    /// held, up to this).
    pub const DEFERRED_GAP_MAX: f64 = 0.5;

    impl Gate {
        /// Waits until the window's clock reaches `until`; `false` when the
        /// window closed.
        fn wait(&self, until: f64) -> bool {
            self.wait_then(until, |_| ()).is_some()
        }

        /// Waits as [`Gate::wait`] does, then reads the state; `None` when
        /// the window closed.
        fn wait_then<T>(&self, until: f64, read: impl FnOnce(&mut GateState) -> T) -> Option<T> {
            let mut g = self.state.lock().ok()?;
            if g.owed > 0.0 {
                g.horizon += std::mem::take(&mut g.owed);
            }
            while g.horizon < until && !g.stopped {
                g.waiting = true;
                g = self.moved.wait(g).ok()?;
            }
            g.waiting = false;
            if g.stopped {
                return None;
            }
            Some(read(&mut g))
        }

        /// The walker's side of the person's mode: waits until the
        /// window's clock reaches `until`, then takes the commands queued
        /// up to the clock, with the clock (every command up to it is
        /// known: later ones are stamped after it). `None`: the window
        /// closed.
        fn take_until(&self, until: f64) -> Option<(Vec<fo4_play::input::Event>, f64)> {
            self.wait_then(until, |g| {
                let n = g.queued.partition_point(|e| e.t <= g.horizon);
                (g.queued.drain(..n).collect(), g.horizon)
            })
        }

        /// The window's side of the person's mode: the window drew `dt`
        /// seconds more ([`Gate::advance`]) and the person gave `commands`
        /// in that frame, at `wall` seconds of the window's wall clock.
        /// They are stamped at the second the walker's clock reaches with
        /// this frame (the horizon, or what the busy run is owed added to
        /// it), which is after every second the walker was told it knows.
        /// In a frame where that clock does not move (the run busy past
        /// what it may be owed, a cell loading) they are kept, in order,
        /// and given when it moves again: spread from that second as they
        /// were made (each gap at most [`DEFERRED_GAP_MAX`]), looks kept to
        /// the last, later commands after them; so a key held across the stand
        /// stays held and a press made during it is neither lost nor
        /// collapsed with its release. Returns the commands stamped, and
        /// that second (the play clock).
        fn submit(
            &self,
            dt: f64,
            wall: f64,
            commands: Vec<fo4_play::input::Command>,
        ) -> (Vec<fo4_play::input::Event>, f64) {
            use fo4_play::input::{Command, Event};
            let mut events = Vec::new();
            let mut clock = 0.0;
            if let Ok(mut g) = self.state.lock() {
                // Stamped under the same lock as the clock moves, before
                // the walker can take the new horizon.
                g.add(dt);
                let t = g.horizon + g.owed;
                if t > g.stamped {
                    let base = t.max(g.floor);
                    let mut before = g.deferred.first().map_or(0.0, |d| d.0);
                    let mut off = 0.0;
                    for (w, command) in std::mem::take(&mut g.deferred) {
                        off += (w - before).clamp(0.0, DEFERRED_GAP_MAX);
                        before = w;
                        events.push(Event {
                            t: base + off,
                            command,
                        });
                    }
                    let at = base + off;
                    events.extend(commands.into_iter().map(|command| Event { t: at, command }));
                    g.floor = at;
                    g.stamped = t;
                    for e in &events {
                        let k = g.queued.partition_point(|q| q.t <= e.t);
                        g.queued.insert(k, *e);
                    }
                } else {
                    for command in commands {
                        // A look made while the clock stands replaces the
                        // one before it.
                        let look = |c: &Command| {
                            std::mem::discriminant(c) == std::mem::discriminant(&command)
                                && matches!(c, Command::Face(_) | Command::Pitch(_))
                        };
                        g.deferred.retain(|(_, c)| !look(c));
                        g.deferred.push((wall, command));
                    }
                }
                clock = t;
            }
            self.moved.notify_all();
            (events, clock)
        }

        /// The window drew `dt` seconds more: the run may go that much
        /// further when it waits, else (busy) when it next waits, up to
        /// [`GATE_BUSY_MAX`].
        fn advance(&self, dt: f64) {
            if let Ok(mut g) = self.state.lock() {
                g.add(dt);
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
        /// `--timeline-cast`: the run's timeline as last read, how many
        /// times it was read, and (reads, seconds) spent reading it.
        pub timeline: Option<Arc<fo4_scene::timeline::Timeline>>,
        pub timeline_gen: u64,
        pub timeline_cost: (u64, f64),
        /// The named platforms' moving parts where the walker's collision
        /// stands (`fo4_play::walker::Walker::platforms`).
        pub platforms: Vec<fo4_play::elevator::Platform>,
        /// The held doors the walker's walking worlds have held so far,
        /// each with whether its box stands now
        /// (`fo4_play::walker::Walker::held_door_states`).
        pub doors: std::collections::BTreeMap<FormKey, bool>,
        /// The collision markers' boxes of the walker's walking world, with
        /// their bounds and whether each stands now
        /// (`fo4_play::walker::Walker::marker_box_states`).
        pub boxes: std::collections::BTreeMap<FormKey, fo4_play::live::MarkerBoxState>,
        /// `--play-keys`: what the activation key would meet from the
        /// walker's eye (`fo4_play::walker::Walker::aim`), as a label.
        pub aim: Option<String>,
        /// Whether that is a thing E uses (not a static's collision or
        /// box, not a collision of no reference): the HUD's prompt.
        pub aim_usable: bool,
        /// The viewport's prompt for it: a short action and a plain name,
        /// from the record's type and display name only (`prompt_posed`).
        pub prompt: Option<String>,
        /// What the person's last activation did, as the viewport confirms
        /// it (opened, used, nothing), numbered so a window shows each once.
        pub confirm: Option<(u64, String)>,
        /// The walker's walking world is being built again (the run is
        /// busy with it): the HUD says loading.
        pub building: bool,
        /// The cell that walking world is being built about: the window
        /// loads its drawing meanwhile.
        pub next_cell: Option<FormKey>,
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
        /// `--timeline-cast`: the run's timeline given to the window when
        /// the run's effects, logs or moves grew.
        timeline: Option<TimelineRead>,
        /// `--play-keys`: the camera's heading from the person's looks (not
        /// the walk's direction), what E would meet given to the window,
        /// and the labels of the references met so far (label, usable,
        /// base record type, display name).
        keys: Option<BTreeMap<FormKey, Label>>,
        /// `--play-keys`: the activations the log held when last read, and
        /// an activation waiting for the run to take it: its line so far
        /// and the run's counts before it ([`Paced::publish_confirm`]).
        confirm: Confirming,
        /// The references whose activation also opens them.
        opens: Vec<FormKey>,
    }

    /// A reference met by the activation: its label, whether E uses it,
    /// its base's record type and display name (the prompt's words).
    type Label = (String, bool, Option<[u8; 4]>, Option<String>);

    /// The activations read, the one waiting for the run (its line so far,
    /// the run's counts before it) and the confirmations shown.
    type Confirming = (usize, Option<(String, (usize, usize, usize))>, u64);

    /// What the run had when its timeline was last read: effects, quest
    /// and scene log entries, native moves; the snapshots taken and their
    /// seconds (wall time).
    #[derive(Default)]
    struct TimelineRead {
        seen: Option<(usize, usize, usize, usize, usize)>,
        /// The run's second of the last read.
        at: Option<f64>,
        taken: u64,
        seconds: f64,
    }

    /// Run seconds between two reads of the run's timeline for the cast at
    /// least (a viewer policy: each read costs about a quarter of a second
    /// on the run's thread in the opening); a change is read at the first
    /// slice after.
    pub const CAST_TIMELINE_EVERY: f64 = 5.0;

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
            let platforms = self.walker.platforms();
            let doors = self.walker.held_door_states();
            let boxes = self.walker.marker_box_states();
            let log = self.log.borrow();
            let Ok(mut s) = self.seen.lock() else {
                return;
            };
            s.platforms = platforms;
            s.doors = doors;
            s.boxes = boxes;
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
            s.heading = if self.keys.is_some() {
                look.map(|l| l.1).or(s.heading)
            } else {
                heading(s.heading, moved, look.map(|l| l.1))
            };
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

    /// The second of the next frame after `after`: the next multiple of
    /// `every`, or sooner the next `FROM + k EVERY` (not past `TO`) of a
    /// stretch `[FROM, TO, EVERY]` of `between`.
    pub fn next_shot(after: f64, every: f64, between: &[[f64; 3]]) -> f64 {
        let mut next = ((after / every).floor() + 1.0) * every;
        for [from, to, step] in between {
            let t = if after < *from {
                *from
            } else {
                from + (((after - from) / step).floor() + 1.0) * step
            };
            if t <= *to && t > after {
                next = next.min(t);
            }
        }
        next
    }

    /// The walker's eye and a point 100 units along its heading and pitch
    /// (the camera's pose), when it stands somewhere.
    pub fn eye_look(s: &Seen) -> Option<([f64; 3], [f64; 3])> {
        let feet = s.feet?;
        let eye = [
            feet[0],
            feet[1],
            feet[2] + fo4_walk::Params::PROVISIONAL.eye_height,
        ];
        let (h, p) = (s.heading?.to_radians(), s.pitch.to_radians());
        let d = [h.cos() * p.cos(), h.sin() * p.cos(), p.sin()];
        Some((eye, std::array::from_fn(|i| eye[i] + 100.0 * d[i])))
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

    impl Paced {
        /// `--timeline-cast`: the run's timeline as it stands, to the
        /// window, when the run did something since the last read. Reading
        /// it changes nothing in the run (`View::timeline`).
        fn publish_timeline(&mut self, run: &View<'_, '_>) {
            let Some(r) = self.timeline.as_mut() else {
                return;
            };
            let w = &run.vm.world;
            let now = (
                run.vm.effects.len(),
                w.quests.log.len(),
                w.scenes.log.len(),
                w.place.native_at.len(),
                w.place.moved_at.len(),
            );
            if r.seen == Some(now) || r.at.is_some_and(|a| run.now() < a + CAST_TIMELINE_EVERY) {
                return;
            }
            r.seen = Some(now);
            r.at = Some(run.now());
            let started = std::time::Instant::now();
            let t = run.timeline();
            r.taken += 1;
            r.seconds += started.elapsed().as_secs_f64();
            if let (Some(t), Ok(mut s)) = (t, self.seen.lock()) {
                s.timeline = Some(Arc::new(t));
                s.timeline_gen += 1;
                s.timeline_cost = (r.taken, r.seconds);
            }
        }
    }

    impl Paced {
        /// `--play-keys`: what the activation key would meet now, labelled
        /// by its base record (type and editor ID) and its key.
        fn publish_aim(&mut self, run: &View<'_, '_>) {
            let Some(labels) = self.keys.as_mut() else {
                return;
            };
            // The held doors' poses as the window draws them: a door
            // whose box left is drawn open (`play_door_drawn_as_held_v1`).
            let doors = self.walker.held_door_states();
            let aim = self.walker.aim().map(|h| match h.reference {
                Some(k) => {
                    let (label, usable, kind, name) =
                        labels.entry(k.clone()).or_insert_with(|| {
                            let b = fo4_core::Budget::new(run.store.limits.max_memory);
                            let base = fo4_vm::bind::base_of(run.store, &k, &b).ok().flatten();
                            let kind = base.as_ref().and_then(|base| run.store.signature(base));
                            // A static's base (the kinds the activatable
                            // object bounds leave out) is not something E uses.
                            let usable =
                                kind.is_some_and(|s| !fo4_play::rules::STATIC_KINDS.contains(&s));
                            let (m, vfs) = run.sources();
                            let name = base.as_ref().and_then(|base| {
                                fo4_play::state::Texts::new(vfs, &m.language, &b)
                                    .display_name(run.store, base)
                            });
                            let what = base
                                .and_then(|base| fo4_vm::bind::label(run.store, &base, &b).ok())
                                .unwrap_or_else(|| "a reference".into());
                            (
                                format!("{what} ({})", fo4_vm::bind::record_key(&k)),
                                usable,
                                kind,
                                name,
                            )
                        });
                    let open = doors.get(&fo4_scene::timeline::norm(&k)) == Some(&false);
                    (
                        format!("{label} at {:.0} units", h.distance),
                        *usable,
                        Some(prompt_posed(*kind, name.as_deref(), open)),
                    )
                }
                None => (
                    format!("collision of no reference at {:.0} units", h.distance),
                    false,
                    None,
                ),
            });
            if let Ok(mut s) = self.seen.lock() {
                s.aim_usable = aim.as_ref().is_some_and(|a| a.1);
                s.prompt = aim.as_ref().and_then(|a| a.2.clone());
                s.aim = aim.map(|a| a.0);
            }
        }
    }

    /// The viewport's prompt for a reference whose base is of `kind` and
    /// named `name` (its display name, when it has one): a short action
    /// by the record's type and the plain name (viewer policy: a door or a
    /// container is opened, anything else used; a door with no name is
    /// named by its type). A door the window draws in its open pose
    /// (`open`: a held door whose box left, `play_door_drawn_as_held_v1`)
    /// is closed: the words follow the drawn pose; what E does to an open
    /// door is the walker's activation as for any door, unchanged. `open`
    /// changes nothing for any other type.
    pub fn prompt_posed(kind: Option<[u8; 4]>, name: Option<&str>, open: bool) -> String {
        let opens = matches!(kind.as_ref(), Some(b"DOOR") | Some(b"CONT"));
        let verb = match (opens, open && kind == Some(*b"DOOR")) {
            (_, true) => "close",
            (true, false) => "open",
            (false, false) => "use",
        };
        match (name.map(str::trim).filter(|n| !n.is_empty()), kind) {
            (Some(n), _) => format!("{verb} {n}"),
            (None, Some(k)) if opens => format!("{verb} {}", String::from_utf8_lossy(&k)),
            (None, _) => verb.to_string(),
        }
    }

    /// The run's counts an activation can change: effects, quest log,
    /// scene log.
    fn run_counts(run: &View<'_, '_>) -> (usize, usize, usize) {
        let w = &run.vm.world;
        (run.vm.effects.len(), w.quests.log.len(), w.scenes.log.len())
    }

    impl Paced {
        /// `--play-keys`: what the person's last activation did, once the
        /// run took the slice it came in: opened (a reference the caller's
        /// opens name), used (the run did something), or nothing (nothing
        /// to use, or the run's effects and logs did not grow). Viewer
        /// policy, read from the run without changing it; nothing to use when
        /// the ray met nothing of a placed reference.
        fn publish_confirm(&mut self, run: &View<'_, '_>) {
            if self.keys.is_none() {
                return;
            }
            let now = run_counts(run);
            let mut line = None;
            if let Some((text, before)) = self.confirm.1.take() {
                line = Some(if text == "opened" {
                    text
                } else if now != before {
                    "used".to_string()
                } else {
                    "nothing happened".to_string()
                });
            }
            let log = self.log.borrow();
            if log.activations.len() > self.confirm.0 {
                let a = &log.activations[log.activations.len() - 1];
                self.confirm.0 = log.activations.len();
                match &a.3 {
                    Ok((k, _)) if self.opens.contains(&fo4_scene::timeline::norm(k)) => {
                        self.confirm.1 = Some(("opened".into(), now));
                    }
                    Ok(_) => self.confirm.1 = Some((String::new(), now)),
                    Err(_) => line = Some("nothing to use".into()),
                }
            }
            drop(log);
            if let Some(l) = line {
                self.confirm.2 += 1;
                if let Ok(mut s) = self.seen.lock() {
                    s.confirm = Some((self.confirm.2, l));
                }
            }
        }
    }

    impl Driver for Paced {
        fn next(&mut self, run: &View<'_, '_>) -> Next {
            self.publish_timeline(run);
            self.publish_aim(run);
            self.publish_confirm(run);
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

    /// The person's commands as the walker takes them (`--play-keys`): it
    /// waits on the window's clock ([`Gate::take_until`]), shows the
    /// window where the walker stands at each wait (each tick, so the
    /// camera follows the walk smoothly) and writes each command it takes
    /// to the record (`--play-record`) as it arrives.
    struct Person {
        gate: Arc<Gate>,
        seen: Arc<Mutex<Seen>>,
        log: Rc<RefCell<fo4_play::walker::Log>>,
        record: Option<std::fs::File>,
        received: u64,
    }

    impl fo4_play::walker::Arrivals for Person {
        fn wait(
            &mut self,
            until: f64,
            at: f64,
            feet: Option<[f64; 3]>,
        ) -> Option<(Vec<fo4_play::input::Event>, f64)> {
            if let (Ok(mut s), Some(f)) = (self.seen.lock(), feet) {
                if s.space.is_none() {
                    s.space = self.log.borrow().start.as_ref().map(|x| x.0.space.clone());
                }
                s.feet = Some(f);
                s.t = s.t.max(at);
            }
            let (events, horizon) = self.gate.take_until(until)?;
            if let Some(f) = self.record.as_mut() {
                use std::io::Write as _;
                for e in &events {
                    if let Err(x) = writeln!(f, "{e}").and_then(|()| f.flush()) {
                        eprintln!("play: --play-record: {x}");
                        self.record = None;
                        break;
                    }
                }
            }
            self.received += events.len() as u64;
            Some((events, horizon))
        }
    }

    impl Drop for Person {
        fn drop(&mut self) {
            println!(
                "play: person: {} commands taken by the walker",
                self.received
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
        input: &crate::cli::Input,
        data: &str,
        out: &str,
        switches: &crate::play_switches::Switches,
        args: Vec<String>,
        track: &fo4_vm::track::Track,
        gate: Arc<Gate>,
        seen: Arc<Mutex<Seen>>,
        status: Sender<Status>,
        with_timeline: bool,
    ) -> Result<BTreeMap<String, u64>, E> {
        use fo4_core::{Budget, Manifest};
        let started = std::time::Instant::now();
        let script = match input {
            crate::cli::Input::Script(file) => {
                fo4_play::input::Script::parse(&std::fs::read_to_string(file)?)?
            }
            crate::cli::Input::Keys(_) => fo4_play::input::Script::default(),
        };
        // `--play-record FILE`: opened before the run starts (an output
        // under ./build, never inside the game's folder).
        let record = match input {
            crate::cli::Input::Keys(crate::cli::Keys {
                record: Some(file), ..
            }) => {
                let mut f = crate::load::output_file(file, Some(std::path::Path::new(data)))?;
                let command: Vec<String> = std::env::args().collect();
                let header = format!(
                    "# the commands the walker received from fo4-view --play-keys, as they arrived (spec/play.md, The person's input)\n# play_run with this file, the same play options (--play-stand among them) and the same new-game switches gives the same run\n# window: {}\n",
                    command.join(" ")
                );
                std::io::Write::write_all(&mut f, header.as_bytes())?;
                Some(f)
            }
            _ => None,
        };
        let m = Manifest::load(std::path::Path::new(manifest))?;
        let store = fo4_plugin::typed::Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
        let vfs = fo4_vfs::Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
        let built = switches.build_world(&m, &store, &vfs, track)?;
        if let Ok(mut s) = seen.lock() {
            s.cell = Some(built.cell.clone());
        }
        let mut report = String::new();
        let mut walker = switches.walker(built, script, &mut report)?;
        drop((store, vfs));
        // The cell a walking world is being built about goes to the window
        // as the build starts, so it loads the cell's drawing meanwhile.
        let told = seen.clone();
        walker = walker.with_on_load(Box::new(move |cell| {
            if let Ok(mut s) = told.lock() {
                match cell {
                    Some(c) => {
                        s.building = true;
                        s.next_cell = Some(c.clone());
                    }
                    None => {
                        s.building = false;
                        if let Some(c) = s.next_cell.take() {
                            s.cell = Some(c);
                        }
                    }
                }
            }
        }));
        let log = walker.log.clone();
        let keys = matches!(input, crate::cli::Input::Keys(_));
        if keys {
            walker = walker.with_arrivals(Box::new(Person {
                gate: gate.clone(),
                seen: seen.clone(),
                log: log.clone(),
                record,
                received: 0,
            }))?;
        }
        let (feed, from_walker) = channel();
        let paced = Paced {
            walker,
            log: log.clone(),
            gate,
            seen,
            link: link::Link::new(from_walker, status),
            feed,
            read: (0, 0, 0),
            timeline: with_timeline.then(TimelineRead::default),
            keys: keys.then(BTreeMap::new),
            confirm: (0, None, 0),
            opens: switches
                .opens
                .iter()
                .map(fo4_scene::timeline::norm)
                .collect(),
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
        fo4_play::report::walker(&l, &mut report);
        let points = fo4_play::report::track_points(track, &l);
        let dir = std::path::Path::new(out);
        std::fs::write(
            dir.join("play-track.txt"),
            fo4_play::report::track_text(&points),
        )?;
        std::fs::write(dir.join("play.txt"), report)?;
        Ok(counts)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Measurement only, with the game data (outputs outside git): a
        /// session's window cell loads replayed headless on the play data
        /// thread as the window asks them. `FO4_VIEW_PLAY_LOADS` names a
        /// local list: `args FILE` (the window's argument files, in order),
        /// `timeline FILE` (the session's run timeline, for the casts),
        /// `lead SECONDS` (how long a cast is built before the cell is
        /// asked, default 0.5) and per cell load `load SECOND CELL DRAWN`
        /// (the run's second, the cell asked and the cell drawn before it,
        /// whose cast is in progress when the cell is asked). Prints per
        /// load when the cast came back and when the cell did.
        #[test]
        #[ignore = "needs the game data and a session's files named by FO4_VIEW_PLAY_LOADS (measurement only)"]
        fn measure_the_windows_cell_loads() {
            let list = std::env::var("FO4_VIEW_PLAY_LOADS").expect("FO4_VIEW_PLAY_LOADS");
            let text = std::fs::read_to_string(&list).unwrap();
            let (mut args, mut timeline, mut lead, mut loads) = (Vec::new(), None, 0.5, Vec::new());
            for line in text.lines() {
                let w: Vec<&str> = line.split_whitespace().collect();
                match w.as_slice() {
                    ["args", f] => args.extend(["--args".to_string(), f.to_string()]),
                    ["timeline", f] => timeline = Some(f.to_string()),
                    ["lead", x] => lead = x.parse::<f64>().unwrap(),
                    ["load", t, cell, drawn] => loads.push((
                        t.parse::<f64>().unwrap(),
                        cell.to_string(),
                        drawn.to_string(),
                    )),
                    _ => {}
                }
            }
            let args = crate::cli::expand_args(&args, &|f| {
                std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))
            })
            .unwrap();
            let crate::cli::Command::Window { scene: s, .. } = crate::cli::parse(&args).unwrap()
            else {
                panic!("not the window's arguments")
            };
            let t = timeline.expect("timeline FILE");
            let tl = Arc::new(
                fo4_scene::timeline::Timeline::parse(&std::fs::read_to_string(t).unwrap()).unwrap(),
            );
            let key = |k: &str| fo4_scene::timeline::parse_key(k).expect("a record key");
            let (data, asks) = channel();
            let (done, from) = channel();
            let (scene, cast) = (s.clone(), s.play_cast.clone());
            let cell_asked = Arc::new(AtomicBool::new(false));
            let asked = cell_asked.clone();
            std::thread::spawn(move || data_worker(scene, cast.map(|a| (a, done)), asks, asked));
            // As the window does: its own load of the first cell, then
            // (`PlayRun::prepare`) the first cell asked again of the play
            // data thread.
            let started = std::time::Instant::now();
            // The window keeps the converted scene, not the draw list.
            let (_drawn, _, first) = crate::load_scene(&s, true, true).unwrap();
            drop(first);
            println!(
                "measure: first load {} in {:.2} s",
                s.cell,
                started.elapsed().as_secs_f64()
            );
            data.send(DataAsk::Prepare(Box::new(s.clone()), true))
                .unwrap();
            // The window's first change of cell comes minutes later.
            std::thread::sleep(std::time::Duration::from_secs_f64(
                std::env::var("FO4_VIEW_PLAY_LOADS_WAIT")
                    .ok()
                    .and_then(|x| x.parse().ok())
                    .unwrap_or(12.0),
            ));
            // `gap`: the cast of the new cell asked as the window asked it
            // before the eleventh-session step, once the cell is back
            // (`FO4_VIEW_PLAY_PROTOCOL=after`), and the seconds from the
            // cell's arrival to its cast's.
            let after = std::env::var("FO4_VIEW_PLAY_PROTOCOL").as_deref() == Ok("after");
            for (second, cell, drawn) in loads {
                if lead > 0.0 {
                    let ask = CastAsk {
                        cell: key(&drawn),
                        timeline: tl.clone(),
                        second,
                        sig: 0,
                    };
                    data.send(DataAsk::Cast(ask)).unwrap();
                    std::thread::sleep(std::time::Duration::from_secs_f64(lead));
                }
                let asked = std::time::Instant::now();
                let mut scene = s.clone();
                scene.cell = cell.clone();
                let (reply, loaded) = channel();
                // The new cell's cast with the cell (the window since the
                // eleventh-session step), unless `after`.
                let with = (!after).then(|| CastAsk {
                    cell: key(&cell),
                    timeline: tl.clone(),
                    second,
                    sig: 1,
                });
                let ask = CellAsk {
                    scene: Box::new(scene),
                    cell: key(&cell),
                    live: true,
                    at: second,
                    started: asked,
                    reply,
                    from: Some(key(&drawn)),
                    cast: with,
                };
                cell_asked.store(true, Ordering::SeqCst);
                data.send(DataAsk::Cell(ask)).unwrap();
                let mut cast_at = (lead <= 0.0).then(|| (0.0, "none asked".to_string()));
                let mut cell_at = None;
                let mut keep = false;
                while cast_at.is_none() || cell_at.is_none() {
                    if let Ok((_, _, r)) = from.try_recv() {
                        let how = match &r {
                            Ok(Some(_)) => "built".to_string(),
                            Ok(None) => "set aside".to_string(),
                            Err(e) => e.clone(),
                        };
                        cast_at = Some((asked.elapsed().as_secs_f64(), how));
                    }
                    if let Ok(r) = loaded.try_recv() {
                        let r = r.unwrap();
                        cell_at = Some(asked.elapsed().as_secs_f64());
                        keep = r.keep_cast;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                let (c, how) = cast_at.unwrap();
                let l = cell_at.unwrap();
                let waited = if c < l { c } else { 0.0 };
                println!(
                    "measure: load {cell} at {second:.3} s: cast of {drawn} back {c:.2} s after the cell's ask ({how}), cell back {l:.2} s after it; waited {waited:.2} s, loaded in {:.2} s",
                    l - waited
                );
                if !after {
                    let r = from.recv().unwrap();
                    println!(
                        "measure: gap {cell} at {second:.3} s: geometry {l:.2} s after the ask, its cast {:.2} s after the geometry ({}); the cast before {}",
                        asked.elapsed().as_secs_f64() - l,
                        if matches!(r.2, Ok(Some(_))) { "built" } else { "not built" },
                        if keep { "stays (one worldspace)" } else { "leaves with its cell" }
                    );
                } else {
                    let back = std::time::Instant::now();
                    let ask = CastAsk {
                        cell: key(&cell),
                        timeline: tl.clone(),
                        second,
                        sig: 0,
                    };
                    data.send(DataAsk::Cast(ask)).unwrap();
                    let r = from.recv().unwrap();
                    println!(
                        "measure: gap {cell} at {second:.3} s: geometry {l:.2} s after the ask, its cast {:.2} s after the geometry ({})",
                        back.elapsed().as_secs_f64(),
                        if matches!(r.2, Ok(Some(_))) { "built" } else { "not built" }
                    );
                }
            }
        }

        /// The cast asked with a cell load and the ones the window asks
        /// after it read one signature: the timeline's cast events up to
        /// the second (a visibility change counts, a stage does not), read
        /// again for a newer snapshot of the timeline.
        #[test]
        fn a_casts_signature_counts_the_cast_events_up_to_its_second() {
            use fo4_scene::timeline::{Event, Timeline, What};
            let key = |id: u32| FormKey {
                plugin: "a.esm".into(),
                object_id: id,
            };
            let shown = |t: f64| Event {
                t,
                what: What::Visibility {
                    reference: key(7),
                    shown: true,
                    nowait: false,
                },
            };
            let stage = |t: f64| Event {
                t,
                what: What::Stage {
                    quest: key(9),
                    stage: 10,
                },
            };
            let (tx, _) = channel();
            let (_, rx) = channel();
            let mut c = CastWork {
                to: tx,
                from: Mutex::new(rx),
                asked: None,
                built: None,
                times: (u64::MAX, Vec::new()),
                builds: 0,
            };
            let t = Timeline {
                events: vec![shown(5.0), stage(6.0), shown(10.0)],
                ..Timeline::default()
            };
            assert_eq!(c.signature(&t, 1, 4.0), 0);
            assert_eq!(c.signature(&t, 1, 5.0), 1);
            assert_eq!(c.signature(&t, 1, 9.0), 1);
            // The same snapshot is not read again; a newer one is.
            let more = Timeline {
                events: vec![shown(5.0), shown(7.0), shown(10.0)],
                ..Timeline::default()
            };
            assert_eq!(c.signature(&more, 1, 9.0), 1);
            assert_eq!(c.signature(&more, 2, 9.0), 2);
        }

        #[test]
        fn the_data_thread_loads_a_cell_first_then_the_newest_cast_then_the_first_cell_again() {
            let line = "m d C --under-hypotheses --walk --eye 0,0,0 --look 1,0,0 --play-run s --play-out o --play-input i --timeline-cast";
            let args: Vec<String> = line.split_whitespace().map(String::from).collect();
            let crate::cli::Command::Window { scene, .. } = crate::cli::parse(&args).unwrap()
            else {
                panic!()
            };
            let key = |id: u32| FormKey {
                plugin: "a.esm".into(),
                object_id: id,
            };
            let cast = |sig: usize| {
                DataAsk::Cast(CastAsk {
                    cell: key(1),
                    timeline: Arc::new(fo4_scene::timeline::Timeline::default()),
                    second: 0.0,
                    sig,
                })
            };
            let (reply, _) = channel();
            let cell = DataAsk::Cell(CellAsk {
                scene: Box::new(scene.clone()),
                cell: key(2),
                live: false,
                at: 0.0,
                started: std::time::Instant::now(),
                reply,
                from: Some(key(1)),
                cast: None,
            });
            let mut queue: std::collections::VecDeque<DataAsk> = [
                cast(1),
                DataAsk::Prepare(Box::new(scene), true),
                cell,
                cast(2),
            ]
            .into();
            let mut prepare = None;
            let what = |a: Option<DataAsk>| match a {
                Some(DataAsk::Cell(c)) => format!("cell {:x}", c.cell.object_id),
                Some(DataAsk::Cast(c)) => format!("cast {}", c.sig),
                Some(DataAsk::Prepare(_, live)) => format!("prepare {live}"),
                None => "none".into(),
            };
            assert_eq!(what(next_ask(&mut queue, &mut prepare)), "cell 2");
            assert!(prepare.is_some());
            assert_eq!(what(next_ask(&mut queue, &mut prepare)), "cast 2");
            assert!(queue.is_empty());
            assert_eq!(what(next_ask(&mut queue, &mut prepare)), "prepare true");
            assert_eq!(what(next_ask(&mut queue, &mut prepare)), "none");
        }

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
        fn commands_made_while_the_clock_stands_are_given_when_it_runs() {
            use fo4_play::input::{Command, Key};
            let gate = Gate::default();
            // The walker waits: the clock runs with the frames.
            gate.state.lock().unwrap().waiting = true;
            let (e, t) = gate.submit(0.5, 0.0, vec![Command::Key(Key::Forward, true)]);
            assert_eq!((e.len(), t), (1, 0.5));
            // The walker is busy (a walking world built again): owed at
            // most GATE_BUSY_MAX, then the clock stands; the person lets W
            // go, taps S (0.4 s) and looks twice while it stands.
            gate.state.lock().unwrap().waiting = false;
            let (e, t) = gate.submit(0.25, 1.0, vec![]);
            assert_eq!((e.len(), t), (0, 0.75));
            for (wall, c) in [
                (
                    2.0,
                    vec![Command::Key(Key::Forward, false), Command::Face(10.0)],
                ),
                (
                    3.0,
                    vec![Command::Key(Key::Back, true), Command::Face(20.0)],
                ),
                (3.4, vec![Command::Key(Key::Back, false)]),
            ] {
                let (e, t) = gate.submit(1.0 / 60.0, wall, c);
                assert!(e.is_empty());
                assert_eq!(t, 0.75);
            }
            // A cell loads (no time), D pressed.
            let (e, _) = gate.submit(0.0, 9.0, vec![Command::Key(Key::Right, true)]);
            assert!(e.is_empty());
            // The run waits again: the clock runs; the commands come in
            // their order, spread as made (each gap at most
            // DEFERRED_GAP_MAX: the tap of S keeps its 0.4 s), the looks
            // kept to the last, this frame's after them.
            gate.state.lock().unwrap().waiting = true;
            let (e, t) = gate.submit(0.1, 9.5, vec![Command::Pitch(-5.0)]);
            assert!((t - 0.85).abs() < 1e-12);
            let got: Vec<(f64, Command)> = e.iter().map(|x| (x.t, x.command)).collect();
            let at = |x: f64, after: f64| (x - 0.85f64 - after).abs() < 1e-9;
            assert_eq!(got.len(), 6, "{got:?}");
            assert!(at(got[0].0, 0.0) && got[0].1 == Command::Key(Key::Forward, false));
            assert!(at(got[1].0, 0.5) && got[1].1 == Command::Key(Key::Back, true));
            assert!(at(got[2].0, 0.5) && got[2].1 == Command::Face(20.0));
            assert!(at(got[3].0, 0.9) && got[3].1 == Command::Key(Key::Back, false));
            assert!(at(got[4].0, 1.4) && got[4].1 == Command::Key(Key::Right, true));
            assert!(at(got[5].0, 1.4) && got[5].1 == Command::Pitch(-5.0));
            // The next frame's commands go after them; the queue is in
            // the order of the seconds.
            let (e, _) = gate.submit(0.1, 9.6, vec![Command::Key(Key::Right, false)]);
            assert!(at(e[0].t, 1.4));
            let q = gate.state.lock().unwrap().queued.clone();
            assert!(q.windows(2).all(|w| w[0].t <= w[1].t));
            assert_eq!(q.len(), 8);
        }

        #[test]
        fn the_exposure_is_measured_from_the_walkers_eye() {
            let mut s = Seen::default();
            assert!(eye_look(&s).is_none());
            s.feet = Some([1.0, 2.0, 3.0]);
            s.heading = Some(90.0);
            let (eye, look) = eye_look(&s).unwrap();
            assert_eq!(eye[2], 3.0 + fo4_walk::Params::PROVISIONAL.eye_height);
            assert!((look[1] - eye[1] - 100.0).abs() < 1e-9 && (look[0] - eye[0]).abs() < 1e-9);
        }

        #[test]
        fn denser_frames_come_between_the_periods() {
            assert_eq!(next_shot(0.0, 30.0, &[]), 30.0);
            let b = [[478.0, 512.0, 2.0], [725.0, 800.0, 2.0]];
            assert_eq!(next_shot(450.0, 30.0, &b), 478.0);
            assert_eq!(next_shot(478.0, 30.0, &b), 480.0);
            assert_eq!(next_shot(480.3, 30.0, &b), 482.0);
            assert_eq!(next_shot(511.0, 30.0, &b), 512.0);
            assert_eq!(next_shot(512.0, 30.0, &b), 540.0);
            assert_eq!(next_shot(720.0, 30.0, &b), 725.0);
            assert_eq!(next_shot(800.0, 30.0, &b), 810.0);
        }

        #[test]
        fn the_prompt_is_a_short_action_and_a_plain_name() {
            assert_eq!(
                prompt_posed(Some(*b"DOOR"), Some("Front Door"), false),
                "open Front Door"
            );
            assert_eq!(prompt_posed(Some(*b"DOOR"), None, false), "open DOOR");
            assert_eq!(prompt_posed(Some(*b"CONT"), Some(" "), false), "open CONT");
            assert_eq!(
                prompt_posed(Some(*b"ACTI"), Some("Console"), false),
                "use Console"
            );
            assert_eq!(prompt_posed(Some(*b"FURN"), None, false), "use");
            assert_eq!(prompt_posed(None, None, false), "use");
            // A door drawn open reads close; the pose changes no other type.
            assert_eq!(
                prompt_posed(Some(*b"DOOR"), Some("Front Door"), true),
                "close Front Door"
            );
            assert_eq!(prompt_posed(Some(*b"DOOR"), None, true), "close DOOR");
            assert_eq!(
                prompt_posed(Some(*b"DOOR"), Some("Front Door"), false),
                "open Front Door"
            );
            assert_eq!(prompt_posed(Some(*b"CONT"), None, true), "open CONT");
            assert_eq!(
                prompt_posed(Some(*b"ACTI"), Some("Console"), true),
                "use Console"
            );
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
    }
}
