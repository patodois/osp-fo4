//! The stepped clock loop: `quest_run`'s clock loop with a horizon. The run
//! only knows the player's track up to the last second the driver gave
//! ([`crate::Slice::until`]); whatever is due later waits until the driver
//! gives the next slice of input (the walker's positions, the player's
//! activations). With no driver installed it is the clock loop itself.
//!
//! Why the result is the run of the whole track (`spec/play.md`, The
//! stepped run): every second the loop picks is the earliest of the
//! timers, the scene clock, the caller's actions and the world model's
//! next due second; the world model's dues before the horizon depend only
//! on the track before it (crossings are folded move by move as
//! `fo4_vm::track::intervals` folds them, distance and load dues read the
//! moves up to the present), so the loop takes the same step with the
//! track known to the horizon as with the whole track, and asks for more
//! input only when the earliest due second lies beyond it.
use super::*;
use crate::{Driver, Next, Slice};
use fo4_vfs::Vfs;
use fo4_vm::track::{self as trk, Crossing, Point, Shape, Track, Volume};
use std::cell::RefCell;
use std::collections::VecDeque;

thread_local! {
    /// The driver of the next run on this thread ([`install`]).
    static DRIVER: RefCell<Option<Box<dyn Driver>>> = const { RefCell::new(None) };
    /// Counts of the last stepped run ([`take_counts`]).
    static COUNTS: RefCell<BTreeMap<String, u64>> = const { RefCell::new(BTreeMap::new()) };
}

/// Installs the driver of the next run on this thread.
pub(crate) fn install(d: Option<Box<dyn Driver>>) {
    DRIVER.with_borrow_mut(|x| *x = d);
}

/// Takes the driver back after the run.
pub(crate) fn take() -> Option<Box<dyn Driver>> {
    DRIVER.with_borrow_mut(Option::take)
}

/// The counts of the last stepped run on this thread.
pub(crate) fn take_counts() -> BTreeMap<String, u64> {
    COUNTS.with_borrow_mut(std::mem::take)
}

/// One volume's intervals over the track's finite moves so far, folded as
/// `fo4_vm::track::intervals` folds them (before its final stay and its
/// filter).
#[derive(Debug, Clone, Default)]
struct Folded(Vec<(f64, f64)>);
impl Folded {
    fn push(
        &mut self,
        v: &Volume,
        body: Option<u32>,
        mv: (f64, f64, &FormKey, [f64; 3], [f64; 3]),
    ) {
        let (t0, t1, space, a, b) = mv;
        if *space != v.space {
            return;
        }
        let span = match body {
            Some(h) => v.span_body(a, b, f64::from(h)),
            None => v.span(a, b),
        };
        let Some((s0, s1)) = span else {
            return;
        };
        let (e, l) = if t1.is_infinite() {
            (t0, f64::INFINITY)
        } else {
            (t0 + s0 * (t1 - t0), t0 + s1 * (t1 - t0))
        };
        match self.0.last_mut() {
            Some(last) if last.1 >= e => last.1 = last.1.max(l),
            _ => self.0.push((e, l)),
        }
    }
    /// The intervals with the stay after the last point, as
    /// `fo4_vm::track::intervals` returns them.
    fn finish(&self, v: &Volume, body: Option<u32>, last: &Point) -> Vec<(f64, f64)> {
        let mut out = self.clone();
        out.push(
            v,
            body,
            (last.t, f64::INFINITY, &last.space, last.pos, last.pos),
        );
        out.0.retain(|(e, l)| l > e);
        out.0
    }
}

/// One move of a track between two points, as `Track::moves` reads it: a
/// move to a point in another space is a stay at the earlier point.
fn step_of<'a>(a: &'a Point, b: &Point) -> (f64, f64, &'a FormKey, [f64; 3], [f64; 3]) {
    if a.space == b.space {
        (a.t, b.t, &a.space, a.pos, b.pos)
    } else {
        (a.t, b.t, &a.space, a.pos, a.pos)
    }
}

/// The hello volume about a placed actor, as `fo4_vm::talk::load_hello`
/// makes it.
fn hello_volume(a: &trk::PlacedActor, d: f64) -> Option<Volume> {
    let frame = fo4_scene::transform::Transform::euler(
        [
            a.pos[0] as f32,
            a.pos[1] as f32,
            a.pos[2] as f32,
            0.0,
            0.0,
            0.0,
        ],
        1.0,
    )
    .ok()?;
    Some(Volume {
        key: a.key.clone(),
        space: a.space.clone(),
        frame,
        shape: Shape::Sphere(d),
    })
}

/// The track as the run knows it and the folds of its crossings.
struct Feed {
    volumes: Vec<Folded>,
    /// Hello volumes by placed actor (`None`: the actor has none), with
    /// their folds; empty when hello lines are off.
    hello: Vec<Option<(Volume, Folded)>>,
    body: Option<u32>,
    /// Points the caller's track had after the run's start (dropped: the
    /// driver's points take their place).
    dropped: usize,
    /// Crossings already applied whose second or order changed when the
    /// track was cut or grown (should stay 0).
    prefix_changed: u64,
    /// The caller's track as the run loaded it.
    loaded: Option<Rc<Track>>,
}
impl Feed {
    /// Keeps the caller's track up to the run's present second (the
    /// start: the play mode's first point) and folds it.
    fn start(vm: &mut Vm<'_>) -> Feed {
        let w = &mut vm.world;
        let now = w.now[fo4_vm::world::Clock::Real as usize];
        let body = w.place.sw.body;
        let mut feed = Feed {
            volumes: vec![Folded::default(); w.place.volumes.len()],
            hello: Vec::new(),
            body,
            dropped: 0,
            prefix_changed: 0,
            loaded: None,
        };
        let Some(track) = w.place.track.clone() else {
            return feed;
        };
        feed.loaded = Some(track.clone());
        let kept: Vec<Point> = track
            .points
            .iter()
            .filter(|p| p.t <= now)
            .cloned()
            .collect();
        feed.dropped = track.points.len() - kept.len();
        if kept.is_empty() {
            return feed;
        }
        if let (Some(d), true) = (w.talk.hello_distance, w.talk.sw.hello && w.place.sw.actors) {
            feed.hello = w
                .place
                .actors
                .iter()
                .map(|a| hello_volume(a, d).map(|v| (v, Folded::default())))
                .collect();
        }
        for pair in kept.windows(2) {
            feed.fold(w, step_of(&pair[0], &pair[1]));
        }
        w.place.track = Some(Rc::new(Track { points: kept }));
        feed.rebuild(w);
        feed
    }
    fn fold(&mut self, w: &World, mv: (f64, f64, &FormKey, [f64; 3], [f64; 3])) {
        for (f, v) in self.volumes.iter_mut().zip(&w.place.volumes) {
            f.push(v, self.body, mv);
        }
        for (v, f) in self.hello.iter_mut().flatten() {
            f.push(v, None, mv);
        }
    }
    /// The crossings of the track as `Place::load_with` and
    /// `talk::load_hello` order them, the cursors kept (the crossings
    /// already applied must not change).
    fn rebuild(&mut self, w: &mut World) {
        let Some(track) = w.place.track.clone() else {
            return;
        };
        let Some(last) = track.points.last() else {
            return;
        };
        let mut crossings = Vec::new();
        for (i, (f, v)) in self.volumes.iter().zip(&w.place.volumes).enumerate() {
            for (e, l) in f.finish(v, self.body, last) {
                crossings.push(Crossing {
                    t: e,
                    volume: i,
                    enter: true,
                });
                if l.is_finite() {
                    crossings.push(Crossing {
                        t: l,
                        volume: i,
                        enter: false,
                    });
                }
            }
        }
        crossings.sort_by(|a, b| {
            a.t.total_cmp(&b.t)
                .then(a.enter.cmp(&b.enter))
                .then(a.volume.cmp(&b.volume))
        });
        let n = w.place.cursor.min(w.place.crossings.len());
        if crossings.get(..n) != Some(&w.place.crossings[..n]) {
            self.prefix_changed += 1;
        }
        w.place.crossings = crossings;
        if self.hello.is_empty() {
            return;
        }
        let mut out = Vec::new();
        for (i, h) in self.hello.iter().enumerate() {
            let Some((v, f)) = h else { continue };
            for (e, l) in f.finish(v, None, last) {
                out.push(fo4_vm::talk::HelloCrossing {
                    t: e,
                    actor: i,
                    enter: true,
                });
                if l.is_finite() {
                    out.push(fo4_vm::talk::HelloCrossing {
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
        let n = w.talk.cursor.min(w.talk.crossings.len());
        if out.get(..n) != Some(&w.talk.crossings[..n]) {
            self.prefix_changed += 1;
        }
        w.talk.crossings = out;
    }
    /// Appends the slice's points (none before the track's last second).
    fn extend(&mut self, w: &mut World, s: &Slice) -> Result<(), E> {
        if s.points.is_empty() {
            return Ok(());
        }
        let Some(rc) = w.place.track.as_mut() else {
            return Err(
                "PlayNoTrack: the play mode needs the run's --track (its first point is the start)"
                    .into(),
            );
        };
        let track = Rc::make_mut(rc);
        let mut prev = track.points.last().cloned();
        for p in &s.points {
            if !p.t.is_finite() || prev.as_ref().is_some_and(|q| p.t < q.t) {
                return Err(format!(
                    "PlayTrackBack: a point at {} s before the track's last second",
                    p.t
                )
                .into());
            }
            prev = Some(p.clone());
        }
        let mut last = track.points.last().cloned();
        let mut moves = Vec::new();
        for p in &s.points {
            if let Some(a) = &last {
                moves.push((a.clone(), p.clone()));
            }
            track.points.push(p.clone());
            last = Some(p.clone());
        }
        for (a, b) in &moves {
            self.fold(w, step_of(a, b));
        }
        self.rebuild(w);
        Ok(())
    }
}

/// The run's timeline so far (`fo4_vm::timeline::Recorder::finish`, with
/// the line texts), when the run writes one (`--timeline`).
fn timeline_now(
    vm: &Vm<'_>,
    store: &Store,
    sources: (&Manifest, &Vfs),
) -> Option<fo4_scene::timeline::Timeline> {
    let (m, vfs) = sources;
    RUN_TIMELINE.with_borrow(|r| {
        r.as_ref().map(|(_, rec)| {
            rec.finish(
                vm,
                store,
                Some((vfs, &m.language)),
                &Budget::new(m.limits.max_memory),
            )
        })
    })
}

/// Adds to the play counts.
fn count(k: impl Into<String>, n: u64) {
    COUNTS.with_borrow_mut(|c| *c.entry(k.into()).or_default() += n);
}

/// With `--cond-1-distance` (`cond_fn_1_track_distance_v1`) a scene whose
/// phase waits on completion conditions with a function 1 condition gets
/// its marks from the track as known when it began to wait; `quest_run`
/// knows the whole track then, the play mode only the track to the
/// horizon. So when the track grows, each such scene with no mark left is
/// offered its completion again (`scene::step_call`), which reads the
/// marks on the longer track. Each offer that finds the conditions still
/// false logs one more wait than `quest_run` does.
fn reoffer_distance_waits(
    vm: &mut Vm<'_>,
    store: &Store,
    names: &mut String,
    top: &mut Vec<JobRun>,
    b: &Budget,
) -> u64 {
    let mut last: BTreeMap<fo4_vm::world::ObjId, Option<usize>> = BTreeMap::new();
    for (o, ch) in &vm.world.scenes.log {
        let p = match ch {
            fo4_vm::scene::Change::Waiting(p, why)
                if why.starts_with("completion conditions false") =>
            {
                Some(*p)
            }
            _ => None,
        };
        last.insert(*o, p);
    }
    let mut offered = 0u64;
    for (o, p) in last {
        let Some(p) = p else { continue };
        if vm.world.scenes.percent_marks.iter().any(|(s, _)| *s == o) {
            continue;
        }
        let Some(key) = vm.world.objects.get(o).and_then(|x| x.key.clone()) else {
            continue;
        };
        let Some(Some(info)) = vm.world.scenes.infos.get(&key).cloned() else {
            continue;
        };
        let distance = info.phases.get(p).is_some_and(|ph| {
            ph.completion
                .iter()
                .any(|x| x.ctda.function == fo4_vm::cond::DISTANCE)
        });
        if !distance {
            continue;
        }
        let _ = vm
            .host
            .call(&fo4_vm::scene::step_call(o, true), &mut vm.world);
        for r in vm.host.rules() {
            vm.rules.insert(r);
        }
        offered += 1;
        let _ = writeln!(
            names,
            "## play: scene {} offered its completion again on the grown track",
            vm.world.objects[o].label
        );
    }
    if offered > 0 {
        count(
            "play: scenes waiting on a distance condition offered their completion again as the track grew",
            offered,
        );
        top.extend(quest::run_jobs(vm, store, MAX_GAME_JOBS, b));
    }
    offered
}

/// `quest_run`'s clock loop, stepped by the installed driver; the clock
/// loop itself when none is installed.
#[allow(clippy::too_many_arguments)]
pub(super) fn play_loop(
    sources: (&Manifest, &Vfs),
    vm: &mut Vm<'_>,
    store: &Store,
    then: &Then,
    scale_global: &Option<FormKey>,
    c: &mut Counts,
    names: &mut String,
    top: &mut Vec<JobRun>,
    b: &Budget,
) -> Result<Vec<(usize, fo4_vm::Val, Outcome)>, E> {
    let Some(mut driver) = take() else {
        return clock_loop(vm, store, then, scale_global, c, names, top, b);
    };
    let out = stepped(
        &mut *driver,
        sources,
        vm,
        store,
        then,
        scale_global,
        c,
        names,
        top,
        b,
    );
    install(Some(driver));
    out
}

/// The clock loop of `quest_run` (same order of what is due, same ties)
/// with the horizon of the input; see the module docs.
#[allow(clippy::too_many_arguments)]
fn stepped(
    driver: &mut dyn Driver,
    sources: (&Manifest, &Vfs),
    vm: &mut Vm<'_>,
    store: &Store,
    then: &Then,
    scale_global: &Option<FormKey>,
    c: &mut Counts,
    names: &mut String,
    top: &mut Vec<JobRun>,
    b: &Budget,
) -> Result<Vec<(usize, fo4_vm::Val, Outcome)>, E> {
    if !vm.world.place.on {
        return Err(
            "PlayNoTrack: the play mode needs the run's --track (its first point is the start)"
                .into(),
        );
    }
    let mut fired = Vec::new();
    let (mut timers, mut steps) = (then.timers, then.scene_clock);
    let mut pending: Vec<Caller> = then
        .actions
        .iter()
        .filter(|x| x.at.is_some())
        .cloned()
        .collect();
    pending.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    let mut pending = VecDeque::from(pending);
    let mut held: Vec<Caller> = Vec::new();
    let mut seen = (0, 0);
    timeline(
        vm,
        &mut seen,
        names,
        "the start and the stages set from the command line",
    );
    let mut feed = Feed::start(vm);
    count(
        "play: points of the caller's track after the start, replaced by the input's",
        feed.dropped as u64,
    );
    let mut horizon = vm.world.now[fo4_vm::world::Clock::Real as usize];
    let mut open = true;
    let mut stopped = false;
    loop {
        if let Some(pos) = held.iter().position(|x| gate_done(vm, store, x, b)) {
            let x = held.remove(pos);
            bump(c, "caller actions held until a stage: happened when the stage was done, after their second");
            act(vm, store, then, &x, c, names, top, b)?;
            world_step(vm, store, then, c, names, top, b);
            timeline(vm, &mut seen, names, "a caller's held action");
            continue;
        }
        let scale = scale_global
            .as_ref()
            .and_then(|g| time_scale(vm, store, g, b));
        let timer = (timers > 0 && !vm.world.timers.is_empty()).then(|| vm.next_timer_wait(scale));
        let due = if steps > 0 {
            scene::next_due(&vm.world)
        } else {
            None
        };
        let now = vm.world.now[fo4_vm::world::Clock::Real as usize];
        let wait = pending
            .front()
            .and_then(|x| x.at)
            .map(|t| (t - now).max(0.0));
        let next_known = match (timer, due) {
            (Some(Some(t)), Some(d)) => Some(t.min(d)),
            (Some(Some(t)), None) => Some(t),
            (None, Some(d)) => Some(d),
            _ => None,
        };
        let stuck = matches!(timer, Some(None));
        let max = store.limits.max_memory;
        let track_wait = [
            fo4_vm::track::next_due(store, max, &mut vm.world),
            fo4_vm::talk::next_due(&vm.world),
            fo4_vm::anim::next_due(&vm.world),
            fo4_vm::load::next_due(store, &vm.world),
        ]
        .into_iter()
        .flatten()
        .min_by(f64::total_cmp)
        .map(|t| (t - now).max(0.0));
        // The play mode: what is due first lies past the input's horizon
        // (or nothing is due): the driver gives the next slice first.
        if open && !stuck {
            let earliest = [next_known, wait, track_wait]
                .into_iter()
                .flatten()
                .min_by(f64::total_cmp)
                .map(|w| now + w);
            if earliest.is_none_or(|t| t > horizon) {
                let view = crate::View {
                    vm,
                    store,
                    sources,
                    loaded: feed.loaded.as_deref(),
                    timeline: &timeline_now,
                };
                match driver.next(&view) {
                    Next::Slice(s) => {
                        if s.until.is_nan() || s.until < horizon {
                            return Err(format!(
                                "PlayHorizonBack: a slice until {} s after the horizon {horizon} s",
                                s.until
                            )
                            .into());
                        }
                        if s.points.last().is_some_and(|p| p.t > s.until) {
                            return Err("PlaySliceBeyond: a point after the slice's second".into());
                        }
                        let moved = !s.points.is_empty();
                        feed.extend(&mut vm.world, &s)?;
                        if moved
                            && then.cond_distance
                            && reoffer_distance_waits(vm, store, names, top, b) > 0
                        {
                            timeline(
                                vm,
                                &mut seen,
                                names,
                                "a scene offered again as the track grew",
                            );
                        }
                        count("play: slices of input", 1);
                        count("play: track points from the input", s.points.len() as u64);
                        horizon = s.until;
                        // In reverse, each before the command line's
                        // actions of its second: the slice's own order is
                        // kept and comes first on a tie.
                        for a in s.actions.into_iter().rev() {
                            if !(a.at >= now && a.at <= s.until) {
                                return Err(format!(
                                    "PlayActionTime: an action at {} s outside the slice ({now} to {} s)",
                                    a.at, s.until
                                )
                                .into());
                            }
                            let key = fo4_vm::bind::record_key(&a.reference);
                            if a.action == fo4_vm::player::Action::Activate
                                && store.signature(&a.reference) == Some(*b"ACHR")
                                && !then.activate_actors
                            {
                                count("play: activations of a placed actor left out (--activate-actors off)", 1);
                                continue;
                            }
                            count(
                                format!(
                                    "play: player actions from the input\t{}",
                                    a.action.handler()
                                ),
                                1,
                            );
                            let x = Caller {
                                what: What::Player(a.action, key),
                                at: Some(a.at),
                                after: None,
                            };
                            // Before the command line's actions of the same
                            // second (the input's action first on a tie).
                            let at = pending
                                .iter()
                                .position(|p| p.at.is_some_and(|t| t >= a.at))
                                .unwrap_or(pending.len());
                            pending.insert(at, x);
                        }
                        continue;
                    }
                    Next::Finish => {
                        open = false;
                        count(
                            "play: input over, the run goes on as with the track's last point held",
                            1,
                        );
                        continue;
                    }
                    Next::Stop => {
                        stopped = true;
                        count("play: input stopped the run", 1);
                        break;
                    }
                }
            }
        }
        if let Some(tw) = track_wait {
            if !stuck && next_known.is_none_or(|n| tw < n) && wait.is_none_or(|wa| tw <= wa) {
                vm.world.now[fo4_vm::world::Clock::Real as usize] = now + tw;
                if let Some(s) = scale {
                    vm.world.now[fo4_vm::world::Clock::GameTime as usize] += tw * s / 3600.0;
                }
                world_step(vm, store, then, c, names, top, b);
                timeline(vm, &mut seen, names, "the player's track");
                continue;
            }
        }
        if let Some(wa) = wait {
            if !stuck && next_known.is_none_or(|n| wa < n) {
                let x = pending.pop_front().expect("a pending action");
                let t = now + wa;
                vm.world.now[fo4_vm::world::Clock::Real as usize] = t;
                if let Some(s) = scale {
                    vm.world.now[fo4_vm::world::Clock::GameTime as usize] += wa * s / 3600.0;
                }
                if x.after.is_some() {
                    if !gate_done(vm, store, &x, b) {
                        let _ = writeln!(
                            names,
                            "## caller's action on {} held at {t} s: its stage is not done",
                            x.target()
                        );
                        held.push(x);
                        continue;
                    }
                    bump(
                        c,
                        "caller actions held until a stage: happened at their second",
                    );
                }
                act(vm, store, then, &x, c, names, top, b)?;
                world_step(vm, store, then, c, names, top, b);
                timeline(vm, &mut seen, names, "a caller's action");
                continue;
            }
        }
        let fire = match (timer, due) {
            (None, None) => break,
            (Some(None), _) => true,
            (Some(Some(t)), Some(d)) => t <= d,
            (Some(Some(_)), None) => true,
            (None, Some(_)) => false,
        };
        if fire {
            let before = vm.effects.len();
            let Some(x) = vm.fire_timer_scaled(scale) else {
                timers = 0;
                continue;
            };
            let _ = writeln!(names, "## timer fired (time scale {scale:?})");
            for e in &vm.effects[before..] {
                let _ = writeln!(
                    names,
                    "    {}{}({}{}) = {}",
                    "  ".repeat(e.depth),
                    e.native,
                    e.receiver
                        .as_ref()
                        .map(|x| format!("on {x}; "))
                        .unwrap_or_default(),
                    e.args.join(", "),
                    e.result
                );
            }
            timers = if matches!(x.2, Outcome::Failed(_)) {
                0
            } else {
                timers - 1
            };
            fired.push(x);
            top.extend(quest::run_jobs(vm, store, MAX_GAME_JOBS, b));
            recheck(vm, store, then, c, top, b);
            world_step(vm, store, then, c, names, top, b);
            timeline(vm, &mut seen, names, "a timer fired");
        } else {
            let t = scene::advance_clock(&mut vm.world, scale);
            vm.rules.insert(fo4_vm::rules::SCENE_CLOCK);
            bump(c, "scene clock advances");
            let _ = writeln!(names, "## scene clock advanced to {t:?} s");
            steps -= 1;
            top.extend(quest::run_jobs(vm, store, MAX_GAME_JOBS, b));
            recheck(vm, store, then, c, top, b);
            world_step(vm, store, then, c, names, top, b);
            timeline(vm, &mut seen, names, "the scene clock advanced");
        }
    }
    count(
        "play: crossings already applied that changed when the track grew (expected 0)",
        feed.prefix_changed,
    );
    if stopped {
        // The window closed: the actions left do not happen.
        count(
            "play: caller actions left when the input stopped the run",
            pending.len() as u64 + held.len() as u64,
        );
        return Ok(fired);
    }
    // Actions left after the clocks stopped, as the clock loop does them.
    loop {
        if let Some(pos) = held.iter().position(|x| gate_done(vm, store, x, b)) {
            let x = held.remove(pos);
            bump(c, "caller actions held until a stage: happened when the stage was done, after their second");
            act(vm, store, then, &x, c, names, top, b)?;
            world_step(vm, store, then, c, names, top, b);
            timeline(vm, &mut seen, names, "a caller's held action");
            continue;
        }
        let Some(x) = pending.pop_front() else { break };
        let now = vm.world.now[fo4_vm::world::Clock::Real as usize];
        let t = x.at.unwrap_or(now).max(now);
        vm.world.now[fo4_vm::world::Clock::Real as usize] = t;
        if x.after.is_some() {
            if !gate_done(vm, store, &x, b) {
                let _ = writeln!(
                    names,
                    "## caller's action on {} held at {t} s: its stage is not done",
                    x.target()
                );
                held.push(x);
                continue;
            }
            bump(
                c,
                "caller actions held until a stage: happened at their second",
            );
        }
        act(vm, store, then, &x, c, names, top, b)?;
        world_step(vm, store, then, c, names, top, b);
        timeline(vm, &mut seen, names, "a caller's action");
    }
    for x in &held {
        bump(
            c,
            "caller actions held until a stage: never happened (the stage was never done)",
        );
        let _ = writeln!(
            names,
            "## caller's action on {} never happened: its stage was never done",
            x.target()
        );
    }
    Ok(fired)
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

    /// The crossings of a whole track as `Place::load_with` makes them.
    fn whole(track: &Track, vols: &[Volume], body: Option<u32>) -> Vec<Crossing> {
        let mut out = Vec::new();
        for (i, v) in vols.iter().enumerate() {
            for (e, l) in trk::intervals(track, v, body) {
                out.push(Crossing {
                    t: e,
                    volume: i,
                    enter: true,
                });
                if l.is_finite() {
                    out.push(Crossing {
                        t: l,
                        volume: i,
                        enter: false,
                    });
                }
            }
        }
        out.sort_by(|a, b| {
            a.t.total_cmp(&b.t)
                .then(a.enter.cmp(&b.enter))
                .then(a.volume.cmp(&b.volume))
        });
        out
    }

    #[test]
    fn folding_a_track_point_by_point_gives_the_whole_tracks_crossings() {
        let space = key(1);
        let other = key(2);
        let frame = |x: f32, y: f32, z: f32, rz: f32| {
            fo4_scene::transform::Transform::euler([x, y, z, 0.0, 0.0, rz], 1.0).unwrap()
        };
        let vols = vec![
            Volume {
                key: key(10),
                space: space.clone(),
                frame: frame(100.0, 0.0, 0.0, 0.3),
                shape: Shape::Box([40.0, 30.0, 50.0]),
            },
            Volume {
                key: key(11),
                space: space.clone(),
                frame: frame(250.0, 40.0, 0.0, 0.0),
                shape: Shape::Sphere(60.0),
            },
            Volume {
                key: key(12),
                space: other.clone(),
                frame: frame(0.0, 0.0, 0.0, 0.0),
                shape: Shape::Box([10.0, 10.0, 10.0]),
            },
        ];
        // A walk in and out of both volumes, a stand, a stand inside, a
        // jump to another space and back, points sharing a second.
        let p = |t: f64, s: &FormKey, x: f64, y: f64| Point {
            t,
            space: s.clone(),
            pos: [x, y, 0.0],
            facing: None,
        };
        let points = vec![
            p(0.0, &space, 0.0, 0.0),
            p(1.0, &space, 0.0, 0.0),
            p(2.0, &space, 110.0, 5.0),
            p(3.0, &space, 110.0, 5.0),
            p(3.0, &space, 112.0, 5.0),
            p(5.0, &space, 300.0, 40.0),
            p(6.0, &other, 0.0, 0.0),
            p(7.0, &other, 50.0, 0.0),
            p(8.0, &space, 250.0, 40.0),
            p(9.0, &space, 400.0, 40.0),
        ];
        for body in [None, Some(120)] {
            let mut folds = vec![Folded::default(); vols.len()];
            for n in 1..=points.len() {
                if n >= 2 {
                    let mv = step_of(&points[n - 2], &points[n - 1]);
                    for (f, v) in folds.iter_mut().zip(&vols) {
                        f.push(v, body, mv);
                    }
                }
                let track = Track {
                    points: points[..n].to_vec(),
                };
                let last = &points[n - 1];
                let mut got = Vec::new();
                for (i, (f, v)) in folds.iter().zip(&vols).enumerate() {
                    for (e, l) in f.finish(v, body, last) {
                        got.push(Crossing {
                            t: e,
                            volume: i,
                            enter: true,
                        });
                        if l.is_finite() {
                            got.push(Crossing {
                                t: l,
                                volume: i,
                                enter: false,
                            });
                        }
                    }
                }
                got.sort_by(|a, b| {
                    a.t.total_cmp(&b.t)
                        .then(a.enter.cmp(&b.enter))
                        .then(a.volume.cmp(&b.volume))
                });
                assert_eq!(got, whole(&track, &vols, body), "prefix {n}, body {body:?}");
            }
            let all = whole(
                &Track {
                    points: points.clone(),
                },
                &vols,
                body,
            );
            assert!(all.len() >= 6, "{all:?}");
        }
    }
}
