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
    use std::collections::{BTreeMap, BTreeSet};
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
        /// The pad read with the keys: the synthetic person's `pad` lines
        /// or `--play-pad` (taken by the window, `crate::pad`).
        pub pad: Option<crate::pad::Wanted>,
        /// The viewport's crosshair and lines (`--play-hud`, on with
        /// `--play-keys`; `crate::hud`).
        pub hud: bool,
    }

    /// `--play-from`: the second the run goes to unpaced, whether the gate
    /// was opened to it, since when the window stood ready there, and
    /// whether the window's pace took over.
    struct PlayFrom {
        at: f64,
        opened: bool,
        ready_since: Option<std::time::Instant>,
        done: bool,
    }

    /// Wall seconds the window holds the play clock at `--play-from`'s
    /// second once its cell is drawn, for the cast and the exposure to come
    /// (viewer policy).
    pub const FROM_SETTLE_SECONDS: f64 = 4.0;

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
        /// `--play-from SECONDS`: the second, whether the gate was opened
        /// to it, and since when the window stood ready there.
        from: Option<PlayFrom>,
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
                    // Measurement only: the load's wall time against the
                    // CPU time this thread ran meanwhile.
                    let (wall0, cpu0) = (
                        std::time::Instant::now(),
                        crate::thread_clock::thread_cpu_seconds(),
                    );
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
                    let cpu = crate::thread_clock::thread_cpu_seconds()
                        .zip(cpu0)
                        .map(|(c1, c0)| c1 - c0);
                    println!(
                        "play: load clocks: {}",
                        crate::thread_clock::clocks_line(wall0.elapsed().as_secs_f64(), cpu)
                    );
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
        let mut pad = p.pad.map(|reset| crate::pad::Wanted::Device { reset });
        let synthetic = match &p.input {
            Some(crate::cli::Input::Keys(crate::cli::Keys {
                synthetic: Some(file),
                ..
            })) => {
                let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
                let (events, states) =
                    crate::person::split_pad(crate::person::parse_synthetic(&text)?);
                if !states.is_empty() {
                    if pad.is_some() {
                        return Err(format!(
                            "{file}: a scripted pad (pad lines) and --play-pad: one of them"
                        ));
                    }
                    println!(
                        "play: a scripted pad (--play-keys-from {file}, test and measurement only): {} pad states read as the pad's",
                        states.len()
                    );
                    pad = Some(crate::pad::Wanted::Fake(crate::pad::FakePad::new(states)));
                }
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
                    from: p.from.map(|at| PlayFrom {
                        at,
                        opened: false,
                        ready_since: None,
                        done: false,
                    }),
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
            pad,
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

        /// `--play-from`: whether the window holds the play clock this
        /// frame. The first call opens the gate to the second (the run goes
        /// there as fast as it computes); the clock is held until the run
        /// is there, the window is `ready` (its cell drawn) and
        /// [`FROM_SETTLE_SECONDS`] have passed, then never again.
        pub fn play_from_holds(&mut self, t: f64, ready: bool) -> bool {
            let Some(s) = self.scripted.as_mut() else {
                return false;
            };
            let Some(f) = s.from.as_mut() else {
                return false;
            };
            if f.done {
                return false;
            }
            if !f.opened {
                s.gate.open_to(f.at);
                f.opened = true;
                println!(
                    "play: --play-from {:.3}: the run goes unpaced to that second",
                    f.at
                );
            }
            // There: the run's clock reached the second, or the run waits at
            // the opened horizon (its last slice may end just short of it).
            let there = t >= f.at || s.gate.waits_at(f.at);
            if !there || !ready {
                f.ready_since = None;
                return true;
            }
            let since = *f.ready_since.get_or_insert_with(std::time::Instant::now);
            if since.elapsed().as_secs_f64() < FROM_SETTLE_SECONDS {
                return true;
            }
            f.done = true;
            println!(
                "play: --play-from {:.3}: the window's pace takes over at {t:.3} s of the run",
                f.at
            );
            false
        }

        /// `--play-from`'s second while the window has not taken over: no
        /// frame is written before it.
        pub fn play_from_pending(&self) -> Option<f64> {
            let f = self.scripted.as_ref()?.from.as_ref()?;
            (!f.done).then_some(f.at)
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

        /// `--draw-cast-settles`: the drawn actors' places whose ground the
        /// window wants (`Seen::ground_asked`); the play thread answers at
        /// its next publish.
        pub fn ask_ground(&self, asks: BTreeMap<FormKey, [f64; 3]>) {
            if let Some(s) = self.scripted.as_ref() {
                if let Ok(mut seen) = s.seen.lock() {
                    if seen.ground_asked != asks {
                        seen.ground_asked = asks;
                    }
                }
            }
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

        /// Whether the run waits for the window with the horizon at
        /// `until` or past it.
        fn waits_at(&self, until: f64) -> bool {
            self.state
                .lock()
                .is_ok_and(|g| g.waiting && g.horizon >= until)
        }

        /// `--play-from`: the run may go up to `until` whatever the
        /// window's clock (the horizon is raised to it, never lowered).
        fn open_to(&self, until: f64) {
            if let Ok(mut g) = self.state.lock() {
                if until.is_finite() && until > g.horizon {
                    g.horizon = until;
                }
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
        /// Where to go when the run has set no stage for a while
        /// ([`cue_line`]).
        pub cue: Option<String>,
        /// The walker's walking world is being built again (the run is
        /// busy with it): the HUD says loading.
        pub building: bool,
        /// The cell that walking world is being built about: the window
        /// loads its drawing meanwhile.
        pub next_cell: Option<FormKey>,
        /// `--draw-cast-settles`: the places of the drawn actors the window
        /// asks the ground under (written by the window,
        /// [`Play::ask_ground`]), and the play thread's answers: for each
        /// actor the place asked and the walking world's highest surface at
        /// or below it raised by `crate::scene::SETTLE_UP` (its height and
        /// the upward part of its normal; `None`: nothing there).
        pub ground_asked: BTreeMap<FormKey, [f64; 3]>,
        pub ground: BTreeMap<FormKey, ([f64; 3], Option<(f64, f64)>)>,
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
        /// the walk's direction).
        keys: Option<BTreeMap<FormKey, Label>>,
        /// What E would meet given to the window, with a person's keys or a
        /// recorded input alike, and the labels of the references met so
        /// far (label, usable, base record type, display name).
        labels: BTreeMap<FormKey, Label>,
        /// The activations the log held when last read (a person's or a
        /// recorded input's), and
        /// an activation waiting for the run to take it: its line so far
        /// and the run's counts before it ([`Paced::publish_confirm`]).
        confirm: Confirming,
        /// The references whose activation also opens them.
        opens: Vec<FormKey>,
        /// The held doors' states at the last look (`confirm_door`).
        confirm_doors: BTreeMap<FormKey, bool>,
        /// An activation read but not yet confirmed ([`confirm_ready`]).
        pending: Option<PendingConfirm>,
        /// The quest log's length and the run's second when it last grew
        /// ([`Paced::publish_cue`]).
        progress: (usize, f64),
        /// The person's activation targets (`--play-activate-person`): the
        /// space each stands in, its bounds' centre and its display name,
        /// read once ([`Paced::publish_cue`]).
        persons_at: Option<PersonsAt>,
        /// The run's effects read so far and the references a script
        /// activated in them ([`Paced::publish_cue`]: a target a script
        /// activated is used).
        scripted: (usize, BTreeSet<FormKey>),
    }

    /// Run seconds with no change of the run's quest log before the
    /// viewport shows where to go (viewer policy).
    pub const CUE_AFTER: f64 = 30.0;

    /// The farthest place the cue names (game units on the ground, to the
    /// volume's nearest point): a viewer policy. The seventeenth session's
    /// hill cue named a place 4,400 to 5,800 units back while going on was
    /// right; the house cue that helped was 186 units away.
    pub const CUE_REACH: f64 = 2048.0;

    /// The cue's volume key, after a tab the viewport does not show (the
    /// HUD cuts at it; the watch log keeps it).
    pub fn cue_tag(key: &str) -> String {
        format!("\t{key}")
    }

    /// The viewport's cue toward the nearest place the run asks about again
    /// (viewer policy, `fo4_vm::track::Place::asking_again`): its distance
    /// on the ground and where it lies from the walker's heading (degrees
    /// from +X towards +Y), from the walker's feet to the place's centre.
    pub fn cue_line(feet: [f64; 3], heading: Option<f64>, to: [f64; 3]) -> String {
        let (dx, dy) = (to[0] - feet[0], to[1] - feet[1]);
        let d = (dx * dx + dy * dy).sqrt();
        let side = match heading {
            Some(h) if d >= 1.0 => {
                let rel = (dy.atan2(dx).to_degrees() - h).rem_euclid(360.0);
                match rel {
                    r if !(45.0..315.0).contains(&r) => "ahead",
                    r if r < 135.0 => "to the left",
                    r if r < 225.0 => "behind you",
                    _ => "to the right",
                }
            }
            _ => "here",
        };
        format!("Go back: {d:.0} units {side}")
    }

    /// How far along the walker's own trail the cue aims (game units of
    /// trail; viewer policy): near enough that the straight line to it
    /// stays on the way the walker came, past a door rather than through a
    /// wall beside it.
    pub const CUE_AIM_TRAIL: f64 = 96.0;

    /// The most trail points the cue looks back over (viewer policy: a
    /// bound on the work of one look; about half an hour of walking).
    pub const CUE_TRAIL_MAX: usize = 20_000;

    /// The farthest apart (game units) the walker's last point before it
    /// left the space and its first on coming back may be for the way back
    /// to pass over the stretch in another space (viewer policy: one door,
    /// left and entered again; the 21st session stepped out of the house
    /// and back and the cue pointed through a wall).
    pub const CUE_DOOR_JOIN: f64 = 128.0;

    /// What the cue adds when it has no way back and the straight line to
    /// the place is not walked (viewer policy).
    pub const CUE_NOT_STRAIGHT: &str = " (not straight: go round)";

    /// Two points of the walker's own trail this near each other on the
    /// ground (game units; two capsule radii of the walk's placeholder) are
    /// joined by the way back [`cue_trail`] gives, what the walker walked
    /// between them left out (viewer policy): nearer than a wall's two
    /// sides can be for a capsule of that radius.
    pub const CUE_CUT: f64 = 32.0;
    /// ... and this near in height (game units; viewer policy): not a
    /// floor above or below.
    pub const CUE_CUT_RISE: f64 = 24.0;

    /// The cue along the way the walker came (viewer policy): back along
    /// the walker's own track (`track`, in time order) in the space
    /// `space` from the feet to the latest point inside the place
    /// (`inside`), the length of that way and the point [`CUE_AIM_TRAIL`]
    /// along it to aim at. Loops are cut: from each point the way goes on
    /// to the earliest-walked point of the trail within [`CUE_CUT`] (and
    /// [`CUE_CUT_RISE`]) of it, so a detour the walker came back from is
    /// left out and the length falls as the walker follows the cue. `None`
    /// when the track in that space holds no point inside the place within
    /// [`CUE_TRAIL_MAX`] points (the caller keeps the straight line). The
    /// track is ground the walker walked, so the aim follows a door or a
    /// corridor where the place's nearest point lies behind a wall.
    #[cfg(test)]
    pub fn cue_trail(
        track: &[fo4_play::Point],
        space: &FormKey,
        feet: [f64; 3],
        inside: impl Fn([f64; 3]) -> bool,
    ) -> Option<([f64; 3], f64)> {
        let way = cue_way(track, space, feet, inside)?;
        Some(way_aim(&way, 0))
    }

    /// The aim [`CUE_AIM_TRAIL`] along `way` from its point `from` and the
    /// length from there to its end.
    fn way_aim(way: &[[f64; 3]], from: usize) -> ([f64; 3], f64) {
        let flat = |a: [f64; 3], b: [f64; 3]| (b[0] - a[0]).hypot(b[1] - a[1]);
        let (mut len, mut aim) = (0.0, None);
        for w in way[from..].windows(2) {
            len += flat(w[0], w[1]);
            if aim.is_none() && len >= CUE_AIM_TRAIL {
                aim = Some(w[1]);
            }
        }
        (aim.unwrap_or(way[way.len() - 1]), len)
    }

    /// The most sight lines one cue tries along the way (viewer policy: a
    /// bound on the work of one look).
    pub const CUE_SIGHT_TRIES: usize = 24;

    /// The cue when the walker can see ahead (viewer policy): `to` is the
    /// place's nearest point, `way` the way back [`cue_way`] gives (from
    /// the feet to the place), `seen` whether the straight line from the
    /// feet to a point is open (the caller asks whether the walker's body
    /// walks it, `fo4_play::Walker::walks_to`). The place's point in sight: straight at it, its distance
    /// on the ground. Else the way's point nearest the place's end in sight
    /// (of at most [`CUE_SIGHT_TRIES`] spread along it): aimed at directly,
    /// the length the straight line to it and the way on from it. Else the
    /// way as [`cue_trail`] gives it. So the way back does not grow while
    /// the walker walks an open line toward the place.
    pub fn cue_sighted(
        feet: [f64; 3],
        to: [f64; 3],
        way: &[[f64; 3]],
        seen: impl Fn([f64; 3]) -> bool,
    ) -> ([f64; 3], f64, bool) {
        let flat = |a: [f64; 3], b: [f64; 3]| (b[0] - a[0]).hypot(b[1] - a[1]);
        if seen(to) {
            return (to, flat(feet, to), true);
        }
        let last = way.len().saturating_sub(1);
        if last >= 1 {
            let step = last.div_ceil(CUE_SIGHT_TRIES).max(1);
            let mut k = last;
            loop {
                if seen(way[k]) {
                    let (_, rest) = way_aim(way, k);
                    return (way[k], flat(feet, way[k]) + rest, true);
                }
                if k <= step {
                    break;
                }
                k -= step;
            }
        }
        let (aim, len) = way_aim(way, 0);
        (aim, len, false)
    }

    /// The way back of [`cue_trail`]: the feet, then the trail's points it
    /// keeps, to the first point inside the place.
    pub fn cue_way(
        track: &[fo4_play::Point],
        space: &FormKey,
        feet: [f64; 3],
        inside: impl Fn([f64; 3]) -> bool,
    ) -> Option<Vec<[f64; 3]>> {
        // The trail back: the feet, then the track newest first, to the
        // first point inside the place.
        let mut pts = vec![feet];
        let mut found = false;
        // A stretch in another space is passed over when the walker left
        // and came back at one door (the trail's two ends within
        // [`CUE_DOOR_JOIN`]); else there is no way back in this space.
        let mut away = false;
        for p in track.iter().rev().take(CUE_TRAIL_MAX) {
            if fo4_scene::timeline::norm(&p.space) != *space {
                away = true;
                continue;
            }
            if away {
                let last = pts[pts.len() - 1];
                if (p.pos[0] - last[0]).hypot(p.pos[1] - last[1]) > CUE_DOOR_JOIN
                    || (p.pos[2] - last[2]).abs() > CUE_DOOR_JOIN
                {
                    return None;
                }
                away = false;
            }
            pts.push(p.pos);
            if inside(p.pos) {
                found = true;
                break;
            }
        }
        if !found {
            return None;
        }
        let cell = |p: [f64; 3]| {
            (
                (p[0] / CUE_CUT).floor() as i64,
                (p[1] / CUE_CUT).floor() as i64,
            )
        };
        let mut grid: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
        for (k, p) in pts.iter().enumerate() {
            grid.entry(cell(*p)).or_default().push(k);
        }
        let flat = |a: [f64; 3], b: [f64; 3]| (b[0] - a[0]).hypot(b[1] - a[1]);
        let near =
            |a: [f64; 3], b: [f64; 3]| flat(a, b) <= CUE_CUT && (b[2] - a[2]).abs() <= CUE_CUT_RISE;
        let last = pts.len() - 1;
        let mut i = 0;
        let mut way = vec![pts[0]];
        while i < last {
            let (cx, cy) = cell(pts[i]);
            let mut j = i + 1;
            for gx in cx - 1..=cx + 1 {
                for gy in cy - 1..=cy + 1 {
                    let Some(ks) = grid.get(&(gx, gy)) else {
                        continue;
                    };
                    // The latest index in this cell near enough.
                    let best = ks
                        .iter()
                        .rev()
                        .take_while(|k| **k > j)
                        .find(|k| near(pts[i], pts[**k]))
                        .copied();
                    if let Some(k) = best {
                        j = k;
                    }
                }
            }
            i = j;
            way.push(pts[i]);
        }
        Some(way)
    }

    /// [`cue_line`] with the distance along the trail [`cue_trail`] gave
    /// and its side from the aim.
    pub fn cue_line_trail(
        feet: [f64; 3],
        heading: Option<f64>,
        aim: [f64; 3],
        length: f64,
    ) -> String {
        let line = cue_line(feet, heading, aim);
        let side = line.split_once(" units ").map_or("here", |(_, s)| s);
        format!("Go back: {length:.0} units {side}")
    }

    /// The person's activation targets by reference: the space each stands
    /// in, its bounds' centre and its display name ([`use_target`]).
    pub type PersonsAt = BTreeMap<FormKey, (FormKey, [f64; 3], Option<String>)>;

    /// The person's activation target the use cue names (viewer policy):
    /// of `at` (each target's space, bounds' centre and name), those in
    /// `space` within [`CUE_REACH`] of the feet on the ground that E has not
    /// met yet (`met`), the nearest.
    pub fn use_target(
        feet: [f64; 3],
        space: &FormKey,
        at: &PersonsAt,
        met: &[FormKey],
    ) -> Option<(FormKey, [f64; 3], Option<String>)> {
        let d = |p: &[f64; 3]| (p[0] - feet[0]).hypot(p[1] - feet[1]);
        at.iter()
            .filter(|(k, (sp, c, _))| sp == space && !met.contains(k) && d(c) <= CUE_REACH)
            .min_by(|a, b| d(&a.1 .1).total_cmp(&d(&b.1 .1)))
            .map(|(k, (_, c, n))| (k.clone(), *c, n.clone()))
    }

    /// The person's activation targets the use cue no longer names (viewer
    /// policy): those E met (`e_met`), those a script activated
    /// (`scripted`: picking up the device's holder activates the device),
    /// and those the run made absent (`absent`: taken or disabled). Once
    /// used, a target stays retired.
    pub fn used_targets(
        e_met: impl Iterator<Item = FormKey>,
        scripted: &BTreeSet<FormKey>,
        absent: impl Fn(&FormKey) -> bool,
        at: &PersonsAt,
    ) -> Vec<FormKey> {
        let mut out: Vec<FormKey> = e_met.collect();
        for k in at.keys() {
            if scripted.contains(&fo4_scene::timeline::norm(k)) || absent(k) {
                out.push(k.clone());
            }
        }
        out
    }

    /// Height (game units) above or below the eye past which the use cue
    /// says so (viewer policy).
    pub const CUE_USE_RISE: f64 = 40.0;

    /// The use cue's words (viewer policy): what to use (its display name,
    /// `(not drawn)` when the window's loads left it out), its distance on
    /// the ground and side from the heading as [`cue_line`] gives them, and
    /// how far below or above the eye (`feet` plus `eye`) it lies.
    pub fn cue_use_line(
        feet: [f64; 3],
        eye: f64,
        heading: Option<f64>,
        centre: [f64; 3],
        name: Option<&str>,
        not_drawn: bool,
    ) -> String {
        let line = cue_line(feet, heading, centre);
        let rest = line.strip_prefix("Go back: ").unwrap_or(&line);
        let rise = centre[2] - (feet[2] + eye);
        let height = if rise < -CUE_USE_RISE {
            format!(", {:.0} below the eye", -rise)
        } else if rise > CUE_USE_RISE {
            format!(", {rise:.0} above the eye")
        } else {
            String::new()
        };
        format!(
            "Use {}{}: {rest}{height}",
            name.unwrap_or("it"),
            if not_drawn { " (not drawn)" } else { "" }
        )
    }

    /// An activation whose confirmation waits for what its press does to
    /// show: the target (`None` when the ray met nothing of a placed
    /// reference), the run's second when it was read, the run's counts and
    /// the held doors' states before the slice it was logged in, and the
    /// subtitle texts the viewport showed then.
    struct PendingConfirm {
        target: Option<FormKey>,
        at: f64,
        before: (usize, usize, usize),
        doors: BTreeMap<FormKey, bool>,
        texts: Vec<String>,
        /// The walker left the door the press met as it stands
        /// ([`confirm_used`]).
        kept: bool,
    }

    /// Run seconds a confirmation waits at most for a door's pose to turn
    /// or a line's text to show (viewer policy): the held doors' reading
    /// takes the person's toggle at the next walking-world update, about a
    /// tenth of a second after the press.
    pub const CONFIRM_WAIT: f64 = 0.5;

    /// A reference met by the activation: its label, whether E uses it,
    /// its base's record type and display name (the prompt's words).
    type Label = (
        String,
        bool,
        Option<[u8; 4]>,
        Option<String>,
        Option<String>,
    );

    /// The activations read, the run's counts at the last look (before the
    /// slice the run took since) and the confirmations shown.
    type Confirming = (usize, (usize, usize, usize), u64);

    /// The words of a press on a door the person's E left as it stands
    /// (`play_door_toggle_blocked_unchanged_v1`): the door is held, so the
    /// confirmation does not say it opened (viewer policy).
    pub const CONFIRM_HELD: &str = "held: left as it stands";

    /// The confirmation of an activation the walker logged in the slice the
    /// run took between two looks at its counts (viewer policy): opened (a
    /// reference the caller's opens name), used (the run's quest or scene
    /// log grew over that slice: a stage or a scene changed), used with no
    /// stage or scene changed (only the scripts' calls grew: the press
    /// reached the target's scripts, which changed no stage or scene) or
    /// nothing happened.
    ///
    /// With `kept` the press met a door the walker left as it stands
    /// ([`fo4_play::walker::Walker::door_kept`]), which says so
    /// ([`CONFIRM_HELD`]) in place of opened, whether or not the caller's
    /// opens name it.
    pub fn confirm_used(
        opened: bool,
        kept: bool,
        before: (usize, usize, usize),
        after: (usize, usize, usize),
    ) -> &'static str {
        if kept {
            CONFIRM_HELD
        } else if opened {
            "opened"
        } else if (after.1, after.2) != (before.1, before.2) {
            "used"
        } else if after != before {
            "used, no stage or scene changed"
        } else {
            "nothing happened"
        }
    }

    /// Whether an activation's confirmation is ready to show (viewer
    /// policy): at once when a held door's pose turned; else, until
    /// [`CONFIRM_WAIT`] passed, it waits while a held door the press met
    /// has not turned yet and nothing else happened, and while a press
    /// that changed a stage or scene has no new subtitle yet and its
    /// target's scripts named nothing they activated (`activated`: the
    /// confirmation can name what the press did at once, so the previous
    /// one does not stay up).
    pub fn confirm_ready(
        door: Option<&str>,
        used: &str,
        door_known: bool,
        new_text: bool,
        activated: bool,
        waited: f64,
    ) -> bool {
        if door.is_some() || waited >= CONFIRM_WAIT {
            return true;
        }
        match used {
            "nothing happened" => !door_known,
            "used" => new_text || activated,
            _ => true,
        }
    }

    /// The confirmation's words (viewer policy): the door's turn, else
    /// what the run did ([`confirm_used`]), with the reference the
    /// target's scripts activated, and, when the press was no door's turn,
    /// the first subtitle that appeared since the press, in the run's own
    /// words: a refusal says why as its line does.
    pub fn confirm_text(
        door: Option<&str>,
        used: &str,
        activated: Option<&str>,
        new_text: Option<&str>,
    ) -> String {
        let mut l = door.unwrap_or(used).to_string();
        if let Some(t) = activated {
            let _ = write!(l, " ({t} activated)");
        }
        if let (None, Some(t)) = (door, new_text) {
            let _ = write!(l, ": \"{t}\"");
        }
        l
    }

    /// The confirmation of a held door the person's E turned
    /// (`--play-door-toggle`): its drawn pose after the slice, open when
    /// its box left (`play_door_drawn_as_held_v1`).
    pub fn confirm_door(was_open: Option<bool>, open: Option<bool>) -> Option<&'static str> {
        match (was_open, open) {
            (Some(a), Some(b)) if a != b => Some(if b { "opened" } else { "closed" }),
            _ => None,
        }
    }

    /// The prompt with the display name of the furniture a scripted
    /// reference links to (viewer policy: `use NAME: LINKED`), the name
    /// read as `linked_furniture_name` reads it.
    pub fn prompt_linked(prompt: String, linked: Option<&str>) -> String {
        match linked.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => format!("{prompt}: {n}"),
            None => prompt,
        }
    }

    /// The display name of the placed furniture a reference with scripts
    /// (a VMAD) links to without a keyword (its XLKR, read as
    /// `vm_host_linked_ref_xlkr_v1` reads it), when there is exactly one
    /// such link and the furniture's base has a name: the thing a scripted
    /// activator hands the activation on to, as the exit's device
    /// activator's script does (the run's own effect log). Viewer policy.
    fn linked_furniture_name(
        store: &fo4_plugin::typed::Store,
        texts: &mut fo4_play::state::Texts<'_>,
        k: &FormKey,
        b: &fo4_core::Budget,
    ) -> Option<String> {
        let (r, plugin) = store.raw_with_plugin(k, b).ok().flatten()?;
        if !r.subrecords.iter().any(|s| s.signature == *b"VMAD") {
            return None;
        }
        let links = fo4_vm::data::linked_refs(store, &r, plugin)?;
        let mut plain = links.into_iter().filter(|(kw, _)| kw.is_none());
        let (Some((_, Some(to))), None) = (plain.next(), plain.next()) else {
            return None;
        };
        let base = fo4_vm::bind::base_of(store, &to, b).ok().flatten()?;
        if store.signature(&base) != Some(*b"FURN") {
            return None;
        }
        texts.display_name(store, &base)
    }

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
            // The ground under the actors the window asks about
            // (`--draw-cast-settles`), asked again at each publish: the
            // walker's world grows and changes as it walks (an answer of
            // nothing given before the squares under an actor loaded
            // would otherwise stand).
            let asked: Vec<(FormKey, [f64; 3])> = s
                .ground_asked
                .iter()
                .map(|(k, p)| (k.clone(), *p))
                .collect();
            for (k, p) in asked {
                let hit = self
                    .walker
                    .ground_below([p[0], p[1], p[2] + crate::scene::SETTLE_UP]);
                s.ground.insert(k, (p, hit));
            }
            let keep: Vec<FormKey> = s.ground_asked.keys().cloned().collect();
            s.ground.retain(|k, _| keep.contains(k));
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
            let labels = &mut self.labels;
            // The held doors' poses as the window draws them: a door
            // whose box left is drawn open (`play_door_drawn_as_held_v1`).
            let doors = self.walker.held_door_states();
            let aim = self.walker.aim().map(|h| match h.reference {
                Some(k) => {
                    let (label, usable, kind, name, linked) =
                        labels.entry(k.clone()).or_insert_with(|| {
                            let b = fo4_core::Budget::new(run.store.limits.max_memory);
                            let base = fo4_vm::bind::base_of(run.store, &k, &b).ok().flatten();
                            let kind = base.as_ref().and_then(|base| run.store.signature(base));
                            // A static's base (the kinds the activatable
                            // object bounds leave out) is not something E uses.
                            let usable =
                                kind.is_some_and(|s| !fo4_play::rules::STATIC_KINDS.contains(&s));
                            let (m, vfs) = run.sources();
                            let mut texts = fo4_play::state::Texts::new(vfs, &m.language, &b);
                            let name = base
                                .as_ref()
                                .and_then(|base| texts.display_name(run.store, base));
                            let linked = linked_furniture_name(run.store, &mut texts, &k, &b);
                            let what = base
                                .and_then(|base| fo4_vm::bind::label(run.store, &base, &b).ok())
                                .unwrap_or_else(|| "a reference".into());
                            (
                                format!("{what} ({})", fo4_vm::bind::record_key(&k)),
                                usable,
                                kind,
                                name,
                                linked,
                            )
                        });
                    let n = fo4_scene::timeline::norm(&k);
                    let open = doors.get(&n) == Some(&false);
                    let prompt = if self.walker.door_kept(&k) {
                        // `play_door_toggle_blocked_unchanged_v1`: E leaves
                        // it as it stands, so the prompt offers no action.
                        prompt_blocked(*kind, name.as_deref())
                    } else {
                        prompt_posed(*kind, name.as_deref(), open)
                    };
                    let prompt = prompt_linked(prompt, linked.as_deref());
                    // A door the held doors' reading has not held is drawn
                    // nowhere (the window shows a door only by that
                    // reading; refused at render readiness otherwise).
                    let unheld_door = *kind == Some(*b"DOOR") && !doors.contains_key(&n);
                    (
                        format!("{label} at {:.0} units", h.distance),
                        *usable,
                        Some(prompt_cued(
                            prompt,
                            unheld_door || crate::load::not_drawn(&k),
                        )),
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

    /// The viewport's prompt for a door the person's activation leaves as
    /// it stands (`play_door_toggle_blocked_unchanged_v1`): its plain name
    /// alone, no action (viewer policy; a door with no name is named by its
    /// type, as [`prompt_posed`] names it).
    pub fn prompt_blocked(kind: Option<[u8; 4]>, name: Option<&str>) -> String {
        match (name.map(str::trim).filter(|n| !n.is_empty()), kind) {
            (Some(n), _) => n.to_string(),
            (None, Some(k)) => String::from_utf8_lossy(&k).into_owned(),
            (None, None) => "door".to_string(),
        }
    }

    /// The prompt with the cue for a target the window does not draw
    /// (viewer policy: the person aims at nothing visible, so the prompt
    /// says so; what E does is unchanged).
    pub fn prompt_cued(prompt: String, not_drawn: bool) -> String {
        if not_drawn {
            format!("{prompt} (not drawn)")
        } else {
            prompt
        }
    }

    /// The display name of the first other placed reference the run's
    /// scripts activated (`ObjectReference.Activate`) among the effects
    /// `from..to`, `target` itself left out: what an activation handed on
    /// (viewer policy, read from the run's effect log).
    fn activated_by_scripts(
        run: &View<'_, '_>,
        target: &FormKey,
        from: usize,
        to: usize,
    ) -> Option<String> {
        let w = &run.vm.world;
        let t = fo4_vm::bind::record_key(target);
        let b = fo4_core::Budget::new(run.store.limits.max_memory);
        let (m, vfs) = run.sources();
        for e in run.vm.effects.get(from..to)? {
            if e.native != "objectreference.activate" {
                continue;
            }
            let Some(k) = e
                .receiver_object
                .and_then(|o| w.objects.get(o))
                .and_then(|o| o.key.as_deref())
                .and_then(fo4_vm::data::key_record)
            else {
                continue;
            };
            if fo4_vm::bind::record_key(&k) == t {
                continue;
            }
            let base = fo4_vm::bind::base_of(run.store, &k, &b).ok().flatten()?;
            return fo4_play::state::Texts::new(vfs, &m.language, &b)
                .display_name(run.store, &base);
        }
        None
    }

    /// The run's counts an activation can change: effects, quest log,
    /// scene log.
    fn run_counts(run: &View<'_, '_>) -> (usize, usize, usize) {
        let w = &run.vm.world;
        (run.vm.effects.len(), w.quests.log.len(), w.scenes.log.len())
    }

    impl Paced {
        /// What the last activation did ([`confirm_used`]), read once the run
        /// took the slice the walker logged it in: the walker logs it as it
        /// builds the slice, so the counts are those of the last look
        /// against now. Viewer policy, read from the run without changing
        /// it; nothing to use when the ray met nothing of a placed
        /// reference.
        fn publish_confirm(&mut self, run: &View<'_, '_>) {
            let now = run_counts(run);
            let before = std::mem::replace(&mut self.confirm.1, now);
            let doors = self.walker.held_door_states();
            let doors_before = std::mem::replace(&mut self.confirm_doors, doors.clone());
            let mut lines = Vec::new();
            let log = self.log.borrow();
            if log.activations.len() > self.confirm.0 {
                let a = &log.activations[log.activations.len() - 1];
                self.confirm.0 = log.activations.len();
                // An earlier one still waiting is shown as it stands.
                if let Some(p) = self.pending.take() {
                    lines.push(self.confirm_words(run, &p, now, &doors));
                }
                let target = a.3.as_ref().ok().map(|(k, _)| k.clone());
                let kept = target.as_ref().is_some_and(|k| self.walker.door_kept(k));
                self.pending = Some(PendingConfirm {
                    kept,
                    target,
                    at: run.now(),
                    before,
                    doors: doors_before,
                    texts: self.link.texts().to_vec(),
                });
            }
            drop(log);
            if let Some(p) = self.pending.take() {
                match &p.target {
                    None => lines.push("nothing to use".to_string()),
                    Some(k) => {
                        let n = fo4_scene::timeline::norm(k);
                        let pose = |m: &BTreeMap<FormKey, bool>| m.get(&n).map(|c| !c);
                        let door = confirm_door(pose(&p.doors), pose(&doors));
                        let used = confirm_used(self.opens.contains(&n), p.kept, p.before, now);
                        let known = p.doors.contains_key(&n) || doors.contains_key(&n);
                        let new_text = self.new_text(&p).is_some();
                        let activated = activated_by_scripts(run, k, p.before.0, now.0).is_some();
                        if confirm_ready(door, used, known, new_text, activated, run.now() - p.at) {
                            lines.push(self.confirm_words(run, &p, now, &doors));
                        } else {
                            self.pending = Some(p);
                        }
                    }
                }
            }
            for l in lines {
                self.confirm.2 += 1;
                if let Ok(mut s) = self.seen.lock() {
                    s.confirm = Some((self.confirm.2, l));
                }
            }
        }

        /// Where to go ([`cue_line`]) when the run's quest log has not grown
        /// for [`CUE_AFTER`] run seconds and a place the walker passed is
        /// asked about again in the walker's space: the nearest. Read from
        /// the run without changing it.
        fn publish_cue(&mut self, run: &View<'_, '_>) {
            let w = &run.vm.world;
            let now = run.now();
            if w.quests.log.len() != self.progress.0 {
                self.progress = (w.quests.log.len(), now);
            }
            // The references a script activated since the last look.
            if let Some(new) = run.vm.effects.get(self.scripted.0..) {
                for e in new {
                    if e.native != "objectreference.activate" {
                        continue;
                    }
                    if let Some(k) = e
                        .receiver_object
                        .and_then(|o| w.objects.get(o))
                        .and_then(|o| o.key.as_deref())
                        .and_then(fo4_vm::data::key_record)
                    {
                        self.scripted.1.insert(fo4_scene::timeline::norm(&k));
                    }
                }
                self.scripted.0 = run.vm.effects.len();
            }
            if self.persons_at.is_none() && now - self.progress.1 >= CUE_AFTER {
                let b = fo4_core::Budget::new(run.store.limits.max_memory);
                let (m, vfs) = run.sources();
                let mut texts = fo4_play::state::Texts::new(vfs, &m.language, &b);
                let mut at = BTreeMap::new();
                for k in self.walker.persons() {
                    let space = fo4_play::world::space_of(run.store, k);
                    let centre = fo4_play::world::bounds_centre(run.store, k, &b);
                    let name = fo4_vm::bind::base_of(run.store, k, &b)
                        .ok()
                        .flatten()
                        .and_then(|base| texts.display_name(run.store, &base));
                    if let (Some(space), Some(centre)) = (space, centre) {
                        at.insert(k.clone(), (fo4_scene::timeline::norm(&space), centre, name));
                    }
                }
                self.persons_at = Some(at);
            }
            let Ok(mut s) = self.seen.lock() else {
                return;
            };
            // The person's activation targets not yet met by E in the
            // walker's space and reach, nearest first: where to use.
            let use_cue = match (s.feet, s.space.as_ref(), self.persons_at.as_ref()) {
                (Some(f), Some(space), Some(at))
                    if !s.building && now - self.progress.1 >= CUE_AFTER =>
                {
                    let space = fo4_scene::timeline::norm(space);
                    let log = self.log.borrow();
                    let e_met = log.activations.iter().filter_map(|a| {
                        a.3.as_ref().ok().map(|(k, _)| fo4_scene::timeline::norm(k))
                    });
                    let walker = &self.walker;
                    let met = used_targets(e_met, &self.scripted.1, |k| walker.made_absent(k), at);
                    let eye = self.walker.eye_height().unwrap_or(0.0);
                    use_target(f, &space, at, &met).map(|(k, centre, name)| {
                        cue_use_line(
                            f,
                            eye,
                            s.heading,
                            centre,
                            name.as_deref(),
                            crate::load::not_drawn(&k),
                        ) + &cue_tag(&fo4_scene::timeline::key_text(&k))
                    })
                }
                _ => None,
            };
            let cue = if now - self.progress.1 < CUE_AFTER {
                None
            } else if use_cue.is_some() {
                use_cue
            } else {
                match (s.feet, s.space.as_ref()) {
                    // Not while the window loads.
                    (Some(f), Some(space)) if !s.building => {
                        let space = fo4_scene::timeline::norm(space);
                        let d = |p: &[f64; 3]| (p[0] - f[0]).hypot(p[1] - f[1]);
                        w.place
                            .asking_again(&w.quests.log)
                            .into_iter()
                            // A volume whose hold changed without an event
                            // (an undetermined enable) is not named.
                            .filter(|v| !w.place.blocked.contains(v))
                            .filter_map(|v| w.place.volumes.get(v))
                            .filter(|v| fo4_scene::timeline::norm(&v.space) == space)
                            // Not one the feet stand in.
                            .filter(|v| !v.contains(f))
                            .map(|v| (v.nearest(f), fo4_scene::timeline::key_text(&v.key), v))
                            .filter(|(to, _, _)| d(to) <= CUE_REACH)
                            .min_by(|a, b| d(&a.0).total_cmp(&d(&b.0)))
                            .map(|(to, key, v)| {
                                // Along the walker's own trail when it
                                // passed the place in this space.
                                let log = self.log.borrow();
                                let line = match cue_way(&log.track, &space, f, |p| v.contains(p)) {
                                    Some(way) => {
                                        // Straight at the place, or at the
                                        // way's latest point, when the
                                        // walker's body walks the straight
                                        // line there (a sight line at eye
                                        // height passed over a bookcase in
                                        // the 21st session).
                                        let walker = &self.walker;
                                        let seen = |p: [f64; 3]| walker.walks_to(p) == Some(true);
                                        let (aim, len, open) = cue_sighted(f, to, &way, seen);
                                        // How the cue was found, for the
                                        // watch only (after a second tab).
                                        let how = match (open, aim == to) {
                                            (true, true) => "walked to the place",
                                            (true, false) => "walked to the way's point",
                                            _ => "along the way",
                                        };
                                        (cue_line_trail(f, s.heading, aim, len), how)
                                    }
                                    // No way back in this space: the
                                    // straight line, said to be no way
                                    // when the walker's body does not
                                    // walk it.
                                    None => match self.walker.walks_to(to) {
                                        Some(false) => (
                                            cue_line(f, s.heading, to) + CUE_NOT_STRAIGHT,
                                            "straight, no way back, not walked",
                                        ),
                                        _ => (cue_line(f, s.heading, to), "straight, no way back"),
                                    },
                                };
                                line.0 + &cue_tag(&key) + "\t" + line.1
                            })
                    }
                    _ => None,
                }
            };
            s.cue = cue;
        }

        /// The first subtitle the viewport shows that it did not show when
        /// the pending activation was read.
        fn new_text(&self, p: &PendingConfirm) -> Option<String> {
            self.link
                .texts()
                .iter()
                .find(|t| !p.texts.contains(t))
                .cloned()
        }

        /// A pending activation's confirmation as it stands now.
        fn confirm_words(
            &self,
            run: &View<'_, '_>,
            p: &PendingConfirm,
            now: (usize, usize, usize),
            doors: &BTreeMap<FormKey, bool>,
        ) -> String {
            let Some(k) = &p.target else {
                return "nothing to use".to_string();
            };
            let n = fo4_scene::timeline::norm(k);
            let pose = |m: &BTreeMap<FormKey, bool>| m.get(&n).map(|c| !c);
            let door = confirm_door(pose(&p.doors), pose(doors));
            let used = confirm_used(self.opens.contains(&n), p.kept, p.before, now);
            let activated = activated_by_scripts(run, k, p.before.0, now.0);
            let text = self.new_text(p);
            confirm_text(door, used, activated.as_deref(), text.as_deref())
        }
    }

    impl Driver for Paced {
        fn next(&mut self, run: &View<'_, '_>) -> Next {
            self.publish_timeline(run);
            self.publish_aim(run);
            self.publish_confirm(run);
            self.publish_cue(run);
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
            labels: BTreeMap::new(),
            confirm: (0, (0, 0, 0), 0),
            confirm_doors: BTreeMap::new(),
            pending: None,
            progress: (0, 0.0),
            persons_at: None,
            scripted: (0, BTreeSet::new()),
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

        /// Measurement only, with the game data (outputs outside git): the
        /// scripted play window's exposure per view at given views of one
        /// cell loaded as the window loads it. `FO4_VIEW_EXPOSURE` names a
        /// local list: `args FILE` (the window's argument files, in order),
        /// `cell KEY` and per view `view EX,EY,EZ LX,LY,LZ`. Prints the
        /// exposure measured on every face as loaded, then with the draws
        /// the window hides at load left out (the references the live world
        /// starts absent, the held doors' draws), then also without the
        /// named platforms' skinned draws (which the window skins again by
        /// the parts' places in the run), and the references and draws
        /// whose faces the eye meets most in the first, and how many of the
        /// points seen no placed light reaches.
        #[test]
        #[ignore = "needs the game data and a local list named by FO4_VIEW_EXPOSURE (measurement only)"]
        fn measure_the_view_exposure() {
            let list = std::env::var("FO4_VIEW_EXPOSURE").expect("FO4_VIEW_EXPOSURE");
            let text = std::fs::read_to_string(&list).unwrap();
            let point = |x: &str| -> [f64; 3] {
                let v: Vec<f64> = x.split(',').map(|v| v.parse().unwrap()).collect();
                [v[0], v[1], v[2]]
            };
            let (mut args, mut cell, mut views) = (Vec::new(), None, Vec::new());
            for line in text.lines() {
                let w: Vec<&str> = line.split_whitespace().collect();
                match w.as_slice() {
                    ["args", f] => args.extend(["--args".to_string(), f.to_string()]),
                    ["cell", c] => cell = Some(c.to_string()),
                    ["view", e, l] => views.push((point(e), point(l))),
                    _ => {}
                }
            }
            let args = crate::cli::expand_args(&args, &|f| {
                std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))
            })
            .unwrap();
            let crate::cli::Command::Window { scene: mut s, .. } =
                crate::cli::parse(&args).unwrap()
            else {
                panic!("not the window's arguments")
            };
            s.cell = cell.expect("cell KEY");
            let (gpu, _, loaded) = crate::load_scene(&s, true, true).unwrap();
            let mut v = gpu.view_exposure.clone().expect("--exposure-eye per view");
            let crate::scene::Seen::Faces(faces) = v.triangles.clone() else {
                panic!("--exposure-eye-visible")
            };
            let draws = v.face_draws.clone();
            let tag_of: BTreeMap<usize, u32> = gpu
                .draws
                .iter()
                .zip(&gpu.instances)
                .map(|(d, i)| (*d, i.tag))
                .collect();
            let live = loaded.live.as_ref().expect("--play-live-world");
            let absent: BTreeSet<&FormKey> = live
                .info
                .iter()
                .filter(|(_, i)| i.piece != 0)
                .map(|(k, _)| k)
                .collect();
            let door_draws: BTreeSet<usize> = loaded.doors.iter().map(|d| d.0).collect();
            let mut hidden = crate::scene::DrawnNow::default();
            for (d, tag) in &tag_of {
                let gone = live
                    .tags
                    .get(tag)
                    .is_some_and(|k| absent.contains(&fo4_scene::timeline::norm(k)));
                if gone || door_draws.contains(d) {
                    hidden.hidden.insert(*d as u32);
                }
            }
            println!(
                "measure: {} faces, {} draws hidden at load ({} of held doors)",
                faces.len(),
                hidden.hidden.len(),
                door_draws.len()
            );
            let t0 = std::time::Instant::now();
            for (eye, look) in views {
                v.set_drawn(crate::scene::DrawnNow::default());
                println!("measure: view {eye:?} {look:?}");
                println!("measure: as loaded: {}", v.measure(eye, look, t0));
                let size = fo4_scene::camera::REFERENCE_SIZE;
                let cam = fo4_scene::camera::look_at(
                    eye,
                    look,
                    fo4_scene::camera::reference_frustum(size),
                    size,
                )
                .unwrap();
                let mut by_ref: BTreeMap<String, (usize, bool)> = BTreeMap::new();
                let mut by_draw: BTreeMap<usize, usize> = BTreeMap::new();
                for (fi, _, _) in fo4_render::seen_visible_of(
                    &faces,
                    &|_| true,
                    &[],
                    &cam,
                    fo4_render::VISIBLE_GRID,
                ) {
                    let d = draws[fi] as usize;
                    *by_draw.entry(d).or_default() += 1;
                    let key = tag_of
                        .get(&d)
                        .and_then(|t| live.tags.get(t))
                        .map_or("-".to_string(), fo4_scene::timeline::key_text);
                    let e = by_ref.entry(key).or_insert((0, false));
                    e.0 += 1;
                    e.1 |= hidden.hidden.contains(&(d as u32));
                }
                let mut top: Vec<_> = by_ref.into_iter().collect();
                top.sort_by_key(|x| std::cmp::Reverse(x.1 .0));
                for (k, (n, h)) in top.iter().take(12) {
                    println!(
                        "measure:   {n} cells {k}{}",
                        if *h { " (hidden at load)" } else { "" }
                    );
                }
                let mut draws_top: Vec<_> = by_draw.into_iter().collect();
                draws_top.sort_by_key(|x| std::cmp::Reverse(x.1));
                for (d, n) in draws_top.iter().take(8) {
                    let at = gpu.draws.iter().position(|x| x == d);
                    let platform = at.and_then(|k| gpu.platforms.iter().find(|p| p.instance == k));
                    let skin = at.is_some_and(|k| gpu.skins.iter().any(|x| x.instance == k));
                    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                    let mut count = 0usize;
                    for (f, fd) in faces.iter().zip(draws.iter()) {
                        if *fd as usize == *d {
                            count += 1;
                            for p in f {
                                for k in 0..3 {
                                    lo[k] = lo[k].min(p[k]);
                                    hi[k] = hi[k].max(p[k]);
                                }
                            }
                        }
                    }
                    println!("measure:   draw {d}: {count} faces within {lo:?} to {hi:?}");
                    println!(
                        "measure:   draw {d}: {n} cells; {}{}",
                        platform.map_or("not a platform's".to_string(), |p| format!(
                            "a platform's shape ({}, chain {:?}, {} bones)",
                            p.model,
                            p.chain,
                            p.bones.len()
                        )),
                        if skin { ", skinned again" } else { "" }
                    );
                }
                v.set_drawn(hidden.clone());
                println!("measure: as drawn at load: {}", v.measure(eye, look, t0));
                // The skinned platform draws the window skins again by
                // their parts: here left out (the parts' places are the
                // run's).
                let mut no_skins = hidden.clone();
                for k in &gpu.skins {
                    if let Some(d) = gpu.draws.get(k.instance) {
                        no_skins.hidden.insert(*d as u32);
                    }
                }
                // What no placed light reaches among the points the eye
                // sees (the light there is the ambient alone), by reference.
                let mut dark: BTreeMap<String, usize> = BTreeMap::new();
                let mut dim: BTreeMap<String, usize> = BTreeMap::new();
                let seen = no_skins.seen(&faces, &draws, &cam, fo4_render::VISIBLE_GRID);
                let keep = |fi: usize| !no_skins.hidden.contains(&draws[fi]);
                let met =
                    fo4_render::seen_visible_of(&faces, &keep, &[], &cam, fo4_render::VISIBLE_GRID);
                for ((fi, _, _), (p, n)) in met.iter().zip(&seen) {
                    let some = v
                        .grid
                        .as_ref()
                        .map_or(&[][..], |g| g.at(*p))
                        .iter()
                        .map(|k| *k as usize);
                    let lit = v.light.at_some(*p, *n, some);
                    let alone = v.light.at_some(*p, *n, std::iter::empty());
                    let key = tag_of
                        .get(&(draws[*fi] as usize))
                        .and_then(|t| live.tags.get(t))
                        .map_or("-".to_string(), fo4_scene::timeline::key_text);
                    if lit.iter().sum::<f32>() / 3. < 0.15 {
                        *dim.entry(key.clone()).or_default() += 1;
                    }
                    if lit == alone {
                        *dark.entry(key).or_default() += 1;
                    }
                }
                let mut dim: Vec<_> = dim.into_iter().collect();
                dim.sort_by_key(|x| std::cmp::Reverse(x.1));
                println!(
                    "measure: {} of {} points seen take a light under 0.15; most on {:?}",
                    dim.iter().map(|x| x.1).sum::<usize>(),
                    seen.len(),
                    &dim[..dim.len().min(4)]
                );
                let mut dark: Vec<_> = dark.into_iter().collect();
                dark.sort_by_key(|x| std::cmp::Reverse(x.1));
                println!(
                    "measure: {} of {} points seen lit by the ambient alone (no placed light reaches); most on {:?}",
                    dark.iter().map(|x| x.1).sum::<usize>(),
                    seen.len(),
                    &dark[..dark.len().min(4)]
                );
                v.set_drawn(no_skins);
                println!(
                    "measure: without the skinned platform draws: {}",
                    v.measure(eye, look, t0)
                );
            }
        }

        /// Measurement only, with the game data (outputs outside git): a
        /// session's window cell loads replayed headless on the play data
        /// thread as the window asks them. `FO4_VIEW_PLAY_LOADS` names a
        /// local list: `args FILE` (the window's argument files, in order),
        /// `timeline FILE` (the session's run timeline, for the casts),
        /// `lead SECONDS` (how long a cast is built before the cell is
        /// asked, default 0.5) and per cell load `load SECOND CELL DRAWN`
        /// (the run's second, the cell asked and the cell drawn before it,
        /// whose cast is in progress when the cell is asked) and, between
        /// them, `cast SECOND CELL` (a cast the window built there, built
        /// and waited for in its place in the list, so the kept files see
        /// the casts' reads as the window's did). Prints per load when the
        /// cast came back and when the cell did.
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
                        Some(drawn.to_string()),
                    )),
                    ["cast", t, cell] => {
                        loads.push((t.parse::<f64>().unwrap(), cell.to_string(), None))
                    }
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
                let Some(drawn) = drawn else {
                    let t0 = std::time::Instant::now();
                    data.send(DataAsk::Cast(CastAsk {
                        cell: key(&cell),
                        timeline: tl.clone(),
                        second,
                        sig: 0,
                    }))
                    .unwrap();
                    let r = from.recv().unwrap();
                    println!(
                        "measure: cast {cell} at {second:.3} s: {} in {:.2} s",
                        if matches!(r.2, Ok(Some(_))) {
                            "built"
                        } else {
                            "not built"
                        },
                        t0.elapsed().as_secs_f64()
                    );
                    continue;
                };
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
        fn play_from_opens_the_gate_to_its_second_and_never_lowers_it() {
            let gate = Gate::default();
            // Busy frames are owed at most GATE_BUSY_MAX; the open is not.
            gate.open_to(745.0);
            assert!(gate.wait(745.0));
            assert!(!gate.waits_at(745.0));
            gate.state.lock().unwrap().waiting = true;
            assert!(gate.waits_at(745.0) && !gate.waits_at(746.0));
            gate.state.lock().unwrap().waiting = false;
            gate.open_to(10.0);
            gate.open_to(f64::NAN);
            assert_eq!(gate.state.lock().unwrap().horizon, 745.0);
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
        fn the_cue_names_a_near_place_with_its_key_for_the_watch() {
            let line = cue_line([0.0, 0.0, 0.0], Some(0.0), [186.0, 0.0, 0.0]) + &cue_tag("a:1");
            assert_eq!(line, "Go back: 186 units ahead\ta:1");
            assert_eq!(line.split('\t').next(), Some("Go back: 186 units ahead"));
            const { assert!(CUE_REACH < 4400.0 && CUE_REACH > 186.0) };
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
            assert_eq!(prompt_cued("use Lamp".into(), true), "use Lamp (not drawn)");
            assert_eq!(
                prompt_blocked(Some(*b"DOOR"), Some("Front Door")),
                "Front Door"
            );
            assert_eq!(prompt_blocked(Some(*b"DOOR"), Some(" ")), "DOOR");
            assert_eq!(prompt_blocked(None, None), "door");
            assert_eq!(prompt_cued("use Lamp".into(), false), "use Lamp");
            // The confirmation compares the counts before and after the
            // slice the activation came in (the run took it between looks).
            assert_eq!(confirm_used(false, false, (1, 2, 3), (1, 3, 3)), "used");
            assert_eq!(
                confirm_used(false, false, (1, 2, 3), (1, 2, 3)),
                "nothing happened"
            );
            assert_eq!(confirm_used(true, false, (1, 2, 3), (1, 2, 3)), "opened");
            // Only the scripts' calls grew: the press reached the target,
            // which changed no stage or scene (a script that refused it).
            assert_eq!(
                confirm_used(false, false, (1, 2, 3), (7, 2, 3)),
                "used, no stage or scene changed"
            );
            assert_eq!(confirm_used(false, false, (1, 2, 3), (7, 2, 4)), "used");
            // A door the walker left as it stands (held, its activation
            // blocked) says so, even when the caller's opens name it and
            // the run's logs grew.
            for after in [(1, 2, 3), (7, 2, 4)] {
                assert_eq!(confirm_used(true, true, (1, 2, 3), after), CONFIRM_HELD);
                assert_eq!(confirm_used(false, true, (1, 2, 3), after), CONFIRM_HELD);
            }
            assert_eq!(confirm_used(true, false, (1, 2, 3), (1, 2, 3)), "opened");
            assert!(confirm_ready(None, CONFIRM_HELD, true, false, false, 0.0));
            assert_eq!(
                confirm_text(None, CONFIRM_HELD, None, None),
                "held: left as it stands"
            );
            // A held door the person's E turned says which way.
            assert_eq!(confirm_door(Some(true), Some(false)), Some("closed"));
            assert_eq!(confirm_door(Some(false), Some(true)), Some("opened"));
            assert_eq!(confirm_door(Some(true), Some(true)), None);
            assert_eq!(confirm_door(None, Some(true)), None);
            // A held door the press met waits for its pose to turn (the
            // held doors' reading takes the toggle after the slice), then
            // says opened or closed; a door that is no held door, or one
            // that never turns, says nothing happened.
            assert!(!confirm_ready(
                None,
                "nothing happened",
                true,
                false,
                false,
                0.1
            ));
            assert!(confirm_ready(
                Some("closed"),
                "nothing happened",
                true,
                false,
                false,
                0.1
            ));
            assert!(confirm_ready(
                None,
                "nothing happened",
                false,
                false,
                false,
                0.0
            ));
            assert!(confirm_ready(
                None,
                "nothing happened",
                true,
                false,
                false,
                CONFIRM_WAIT
            ));
            assert_eq!(
                confirm_text(Some("closed"), "nothing happened", None, None),
                "closed"
            );
            // A stage the press set waits for the line it starts, whose
            // text the confirmation carries in the run's own words.
            assert!(!confirm_ready(None, "used", false, false, false, 0.2));
            assert!(confirm_ready(None, "used", false, true, false, 0.2));
            // The scripts named what the press activated: at once (the
            // exit console after the device, seventeenth session).
            assert!(confirm_ready(None, "used", false, false, true, 0.2));
            assert!(confirm_ready(
                None,
                "used",
                false,
                false,
                false,
                CONFIRM_WAIT
            ));
            assert_eq!(
                confirm_text(None, "used", None, Some("Needs the key. Have a nice day.")),
                "used: \"Needs the key. Have a nice day.\""
            );
            assert_eq!(
                confirm_text(None, "used", Some("Lever"), None),
                "used (Lever activated)"
            );
            // The cue toward a place: its distance on the ground and its
            // side from the heading (+X is heading 0, +Y is 90).
            assert_eq!(
                cue_line([0.0, 0.0, 0.0], Some(0.0), [400.0, 300.0, 50.0]),
                "Go back: 500 units ahead"
            );
            assert_eq!(
                cue_line([0.0; 3], Some(0.0), [0.0, 200.0, 0.0]),
                "Go back: 200 units to the left"
            );
            assert_eq!(
                cue_line([0.0; 3], Some(90.0), [0.0, -200.0, 0.0]),
                "Go back: 200 units behind you"
            );
            assert_eq!(
                cue_line([0.0; 3], Some(90.0), [200.0, 0.0, 0.0]),
                "Go back: 200 units to the right"
            );
            assert_eq!(
                cue_line([0.0; 3], None, [200.0, 0.0, 0.0]),
                "Go back: 200 units here"
            );
            // Along the trail: the walker came from a room at x < 0
            // through a door at (0, 100) and stands at (100, 0); the
            // place's nearest point lies straight ahead behind a wall, the
            // trail leads to the door first.
            let space = FormKey {
                plugin: "a.esm".into(),
                object_id: 7,
            };
            let other = FormKey {
                plugin: "a.esm".into(),
                object_id: 8,
            };
            let pt = |x: f64, y: f64, space: &FormKey| fo4_play::Point {
                t: 0.0,
                space: space.clone(),
                pos: [x, y, 0.0],
                facing: None,
            };
            let room = |p: [f64; 3]| p[0] < 0.0;
            let track = vec![
                pt(-50.0, 100.0, &space),
                pt(0.0, 100.0, &space),
                pt(50.0, 50.0, &space),
                pt(100.0, 0.0, &space),
            ];
            let (aim, len) = cue_trail(&track, &space, [100.0, 0.0, 0.0], room).unwrap();
            assert_eq!(aim, [0.0, 100.0, 0.0]);
            assert!(
                (len - (2.0 * 50f64.hypot(50.0) + 50.0)).abs() < 1e-9,
                "{len}"
            );
            // Facing +Y the door is ahead (the place's straight line, -X,
            // would be to the left).
            assert_eq!(
                cue_line_trail([100.0, 0.0, 0.0], Some(135.0), aim, len),
                "Go back: 191 units ahead"
            );
            // Another space between, left and entered again at one door:
            // passed over, the trail as before.
            let mut cut = track.clone();
            cut.insert(2, pt(0.0, 0.0, &other));
            assert_eq!(
                cue_trail(&cut, &space, [100.0, 0.0, 0.0], room),
                Some((aim, len))
            );
            // Left at one door and back at another far from it: no trail,
            // the straight line stays.
            let far = vec![
                pt(-50.0, 100.0, &space),
                pt(0.0, 0.0, &other),
                pt(300.0, -300.0, &space),
                pt(100.0, 0.0, &space),
            ];
            assert_eq!(cue_trail(&far, &space, [100.0, 0.0, 0.0], room), None);
            // A short trail aims at the first point inside.
            assert_eq!(
                cue_trail(&track[..1], &space, [-10.0, 100.0, 0.0], room),
                Some(([-50.0, 100.0, 0.0], 40.0))
            );
            assert_eq!(cue_trail(&[], &space, [0.0; 3], room), None);
            // A detour the walker came back from is cut: out of the door at
            // (0, 100), east to (200, 100), north to (200, 300) and back
            // to (200, 110), east to (300, 100). The way back is 300 units,
            // not 680, and it falls as the walker follows it.
            let mut walk = vec![pt(-20.0, 100.0, &space)];
            let mut leg = |from: [f64; 2], to: [f64; 2]| {
                for k in 1..=10 {
                    let f = f64::from(k) / 10.0;
                    let (x, y) = (
                        from[0] + (to[0] - from[0]) * f,
                        from[1] + (to[1] - from[1]) * f,
                    );
                    walk.push(pt(x, y, &space));
                }
            };
            leg([-20.0, 100.0], [200.0, 100.0]);
            leg([200.0, 100.0], [200.0, 300.0]);
            leg([200.0, 300.0], [200.0, 110.0]);
            leg([200.0, 110.0], [300.0, 100.0]);
            let (_, len) = cue_trail(&walk, &space, [300.0, 100.0, 0.0], room).unwrap();
            assert!(len < 330.0, "{len}");
            walk.extend([pt(250.0, 100.0, &space), pt(200.0, 100.0, &space)]);
            let (aim, len2) = cue_trail(&walk, &space, [200.0, 100.0, 0.0], room).unwrap();
            assert!(len2 < len - 90.0, "{len2} after {len}");
            // Aimed back along the corridor, not up the detour.
            assert!(aim[1] < 150.0 && aim[0] < 200.0, "{aim:?}");
            // The use cue: the nearest target not yet met, in the space and
            // reach; its words give the side and the height from the eye.
            let k = |id: u32| FormKey {
                plugin: "base.esm".into(),
                object_id: id,
            };
            let mut at = BTreeMap::new();
            at.insert(
                k(1),
                (space.clone(), [40.0, 0.0, 10.0], Some("Device".to_string())),
            );
            at.insert(k(2), (space.clone(), [10.0, 0.0, 10.0], None));
            at.insert(k(3), (k(9), [5.0, 0.0, 10.0], None));
            at.insert(k(4), (space.clone(), [5000.0, 0.0, 10.0], None));
            let feet = [0.0, 0.0, 0.0];
            assert_eq!(use_target(feet, &space, &at, &[]).map(|t| t.0), Some(k(2)));
            let (t, c, n) = use_target(feet, &space, &at, &[k(2)]).unwrap();
            assert_eq!(t, k(1));
            assert_eq!(
                cue_use_line(feet, 120.0, Some(0.0), c, n.as_deref(), true),
                "Use Device (not drawn): 40 units ahead, 110 below the eye"
            );
            assert_eq!(use_target(feet, &space, &at, &[k(1), k(2)]), None);
            // Retired once used: a script activated the device (its holder
            // was picked up), so with E only on the holder (k(7), not a
            // target) nothing is named; a target the run made absent is
            // retired too, and E's own targets stay retired.
            let scripted: BTreeSet<FormKey> = [k(1)].into_iter().collect();
            let used = used_targets([k(7)].into_iter(), &scripted, |_| false, &at);
            assert_eq!(
                use_target(feet, &space, &at, &used).map(|t| t.0),
                Some(k(2))
            );
            let used = used_targets([k(2)].into_iter(), &scripted, |_| false, &at);
            assert_eq!(use_target(feet, &space, &at, &used), None);
            let used = used_targets(std::iter::empty(), &BTreeSet::new(), |x| *x == k(2), &at);
            assert_eq!(
                use_target(feet, &space, &at, &used).map(|t| t.0),
                Some(k(1))
            );
            let used = used_targets(std::iter::empty(), &BTreeSet::new(), |_| false, &at);
            assert_eq!(
                use_target(feet, &space, &at, &used).map(|t| t.0),
                Some(k(2))
            );
            // In sight: the walker walked from the room's door (0, 100)
            // east to (300, 100) and turns back west toward the room. The
            // way back is the corridor; with the room's point in sight the
            // cue points at it directly and its length is the straight
            // line, which falls step by step as the walker walks toward it.
            let way = cue_way(&walk, &space, [300.0, 100.0, 0.0], room).unwrap();
            let to = [0.0, 100.0, 0.0];
            let open = |_: [f64; 3]| true;
            let (aim, len, sighted) = cue_sighted([300.0, 100.0, 0.0], to, &way, open);
            assert!(sighted && aim == to && (len - 300.0).abs() < 1e-9);
            let mut last = len;
            for x in [260.0, 220.0, 180.0, 140.0] {
                let (_, l, _) = cue_sighted([x, 100.0, 0.0], to, &way, open);
                assert!(l < last, "{l} after {last}");
                last = l;
            }
            // The place hidden (a wall at x < 100), a way point east of the
            // wall in sight: aimed at the latest such point, the length the
            // straight line to it and the way on from it.
            let east = |p: [f64; 3]| p[0] >= 100.0;
            let (aim, len, sighted) = cue_sighted([300.0, 100.0, 0.0], to, &way, east);
            assert!(sighted && aim[0] >= 100.0 && aim[0] < 130.0, "{aim:?}");
            assert!(len < 330.0, "{len}");
            // Nothing in sight: the way as cue_trail gives it.
            let (aim, len, sighted) = cue_sighted([300.0, 100.0, 0.0], to, &way, |_| false);
            assert_eq!(
                (aim, len),
                cue_trail(&walk, &space, [300.0, 100.0, 0.0], room).unwrap()
            );
            assert!(!sighted);
            // A door's turn carries no line.
            assert_eq!(
                confirm_text(Some("opened"), "used", None, Some("Hi.")),
                "opened"
            );
            assert!(confirm_ready(None, "opened", false, false, false, 0.0));
            // A scripted activator's linked furniture is named after it.
            assert_eq!(
                prompt_linked("use Crate".into(), Some("Seat")),
                "use Crate: Seat"
            );
            assert_eq!(prompt_linked("use Lamp".into(), Some(" ")), "use Lamp");
            assert_eq!(prompt_linked("use Lamp".into(), None), "use Lamp");
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
