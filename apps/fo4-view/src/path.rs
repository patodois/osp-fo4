//! `--headless --walk --terrain --around N --path X,Y;...`: the moving grid
//! without a window, for tests and measurements. The walker is settled
//! from `--eye` and walked to each waypoint in turn at the run speed, one
//! fixed tick at a time; whenever its feet enter another grid cell the
//! loader drops and loads cells as the window's loading thread would, but
//! synchronously, before the next tick. Every grid change is printed with
//! the cells dropped and loaded, the time each took and the credits kept;
//! the frame is drawn from where the walk ends.
use crate::cells::{self, Message, Streamer};
use crate::cli;
use crate::fly::FlyState;
use crate::live::Live;
use crate::load::{self, AnyResult};
use crate::stream::{self, Policy};
use crate::walk::WalkView;
use fo4_render::RenderOptions;
use fo4_walk::{Input, Params, Settings, WorldSet};
use std::time::Instant;

fn mib(n: u64) -> f64 {
    n as f64 / f64::from(1u32 << 20)
}

/// Cells dropped and loaded for one grid change.
#[derive(Default)]
struct Change {
    dropped: Vec<[i32; 2]>,
    loaded: Vec<[i32; 2]>,
    worker: f64,
    receiving: f64,
}

/// Drops and loads what `centre` asks for, applying each message to `live`
/// and `space` as the window does, and prints one line per cell loaded.
fn change(
    s: &mut Streamer<'_>,
    centre: Option<[i32; 2]>,
    live: &mut Live,
    space: &mut WorldSet<[i32; 2]>,
) -> AnyResult<Change> {
    let mut out = Change::default();
    let mut apply = |m: Message, out: &mut Change| -> AnyResult<()> {
        let t = Instant::now();
        match m {
            Message::Drop { grid, .. } => {
                live.drop_cell(grid, space);
                out.dropped.push(grid);
            }
            Message::Load(piece) => {
                let r = piece.report.clone();
                let grid = piece.grid;
                live.receive(*piece, space)?;
                let ms = t.elapsed().as_secs_f64() * 1e3;
                println!(
                    "  load {},{} {}: {:.3} s worker ({}), {:.2} ms received; {} placements, {} omitted, terrain {}, {} triangles; {} new assets ({:.1} MiB), {} shared; credits {:.1} MiB",
                    grid[0],
                    grid[1],
                    r.outcome,
                    r.seconds,
                    crate::cells::Phases::line(&r.phases),
                    ms,
                    r.placements,
                    r.omitted,
                    r.terrain,
                    r.triangles,
                    r.new_assets,
                    mib(r.new_bytes),
                    r.shared_assets,
                    mib(r.credits),
                );
                if let Some(a) = &r.actors {
                    println!("    {a}");
                }
                out.loaded.push(grid);
                out.worker += r.seconds;
            }
            Message::Failed(e) => return Err(e.into()),
            Message::Ready(_) => {}
        }
        out.receiving += t.elapsed().as_secs_f64();
        Ok(())
    };
    if let Some(c) = centre {
        for m in s.move_to(c) {
            apply(m, &mut out)?;
        }
    }
    if s.pending() {
        let mut loader = s.loader();
        while let Some(m) = s.load_next(&mut loader)? {
            apply(m, &mut out)?;
        }
    }
    Ok(out)
}

fn grids(v: &[[i32; 2]]) -> String {
    let parts: Vec<String> = v.iter().map(|g| format!("{},{}", g[0], g[1])).collect();
    format!("[{}]", parts.join(" "))
}

/// Ticks a leg may take: four times the straight run plus two seconds.
fn leg_cap(distance: f64, speed: f64, tick: f64) -> u64 {
    (4.0 * distance / (speed * tick)).ceil() as u64 + 120
}

/// A move has stalled when its distance shrank by less than this over
/// [`STALL_TICKS`] ticks.
pub(crate) const STALL_UNITS: f64 = 0.5;
pub(crate) const STALL_TICKS: u64 = 60;
/// A stalled leg steps aside and tries again: to the left of the line to
/// its waypoint by [`DETOUR_UNITS`], then to the right by twice that, and
/// so on, at most [`DETOURS`] times. A property of this scripted walk, not
/// of the controller or of the game.
pub(crate) const DETOUR_UNITS: f64 = 512.0;
pub(crate) const DETOURS: u32 = 8;

/// The walker and the moving grid around it.
struct Run<'a> {
    streamer: Streamer<'a>,
    live: Live,
    space: WorldSet<[i32; 2]>,
    view: WalkView,
    centre: [i32; 2],
    heading: f64,
    ticks: u64,
    airborne: u64,
    crossings: u32,
    most_credits: u64,
}

impl Run<'_> {
    /// Walks straight to `target` at the run speed for at most `cap`
    /// ticks: `reached`, `stalled` or `timed_out`.
    fn walk_to(&mut self, target: [f64; 2], cap: u64) -> AnyResult<&'static str> {
        let p = self.view.character.params;
        let tick = self.view.character.settings.tick;
        let (mut n, mut best, mut best_at) = (0u64, f64::INFINITY, 0u64);
        loop {
            let f = self.view.character.feet;
            let (dx, dy) = (target[0] - f[0], target[1] - f[1]);
            let d = dx.hypot(dy);
            if d < 1.0 {
                return Ok("reached");
            }
            if n >= cap {
                return Ok("timed_out");
            }
            if d < best - STALL_UNITS {
                best = d;
                best_at = n;
            } else if n - best_at >= STALL_TICKS {
                return Ok("stalled");
            }
            self.heading = dy.atan2(dx);
            let one = p.run_speed * tick;
            self.view.character.tick(
                &self.space,
                Input {
                    forward: (d / one).min(1.0),
                    right: 0.0,
                    heading: self.heading,
                    run: true,
                },
            );
            self.view.recover(&self.space, tick);
            n += 1;
            self.ticks += 1;
            if !self.view.character.grounded {
                self.airborne += 1;
            }
            self.follow()?;
        }
    }

    /// Moves the grid when the feet entered another cell.
    fn follow(&mut self) -> AnyResult<()> {
        let c = &self.view.character;
        let f = c.feet;
        let Some(here) = stream::cell_at(f[0], f[1]) else {
            return Ok(());
        };
        if here == self.centre {
            return Ok(());
        }
        self.crossings += 1;
        let before = self.streamer.credits();
        let t = Instant::now();
        let ch = change(
            &mut self.streamer,
            Some(here),
            &mut self.live,
            &mut self.space,
        )?;
        self.most_credits = self.most_credits.max(self.streamer.credits());
        // Is there ground under the feet in the new grid?
        let under = self.space.ray_down(f[0], f[1], f[2] + 1.0);
        println!(
            "crossing {} at tick {}, feet {:.1},{:.1},{:.1} ({}, surface {}): {},{} -> {},{}; dropped {} {}; loaded {} {} in {:.2} s worker, {:.1} ms receiving, {:.2} s in all; credits kept {:.1} -> {:.1} MiB; {} cells kept, {} shared assets, {} collision pieces",
            self.crossings,
            self.ticks,
            f[0],
            f[1],
            f[2],
            if c.grounded { "on ground" } else { "in air" },
            under.map_or("none".to_string(), |h| format!("{:.1} below", f[2] - h.z)),
            self.centre[0],
            self.centre[1],
            here[0],
            here[1],
            ch.dropped.len(),
            grids(&ch.dropped),
            ch.loaded.len(),
            grids(&ch.loaded),
            ch.worker,
            ch.receiving * 1e3,
            t.elapsed().as_secs_f64(),
            mib(before),
            mib(self.streamer.credits()),
            self.streamer.kept().count(),
            self.streamer.shared_assets(),
            self.space.len(),
        );
        self.centre = here;
        Ok(())
    }

    /// One leg: straight to `target`, stepping aside when stalled
    /// ([`DETOURS`]).
    fn leg(&mut self, target: [f64; 2]) -> AnyResult<(&'static str, u32)> {
        let p = self.view.character.params;
        let tick = self.view.character.settings.tick;
        let mut detours = 0;
        loop {
            let f = self.view.character.feet;
            let distance = (target[0] - f[0]).hypot(target[1] - f[1]);
            let outcome = self.walk_to(target, leg_cap(distance, p.run_speed, tick))?;
            if outcome != "stalled" || detours == DETOURS {
                return Ok((outcome, detours));
            }
            detours += 1;
            // Left of the line to the target, then right, further each time.
            let f = self.view.character.feet;
            let (dx, dy) = (target[0] - f[0], target[1] - f[1]);
            let d = dx.hypot(dy).max(1e-9);
            let side = if detours % 2 == 1 { 1.0 } else { -1.0 };
            let reach = DETOUR_UNITS * f64::from(detours) * side;
            let aside = [f[0] - dy / d * reach, f[1] + dx / d * reach];
            let cap = leg_cap(reach.abs(), p.run_speed, tick);
            self.walk_to(aside, cap)?;
        }
    }
}

pub fn headless(
    s: &cli::Scene,
    size: [u32; 2],
    out: &str,
    path: &[[f64; 2]],
    start_pose: bool,
) -> AnyResult<()> {
    let started = Instant::now();
    fo4_walk::accept(&load::walk_rules(), s.level, fo4_scene::hypotheses::status)?;
    // The cell under the walker is read under the terrain placement rule.
    fo4_walk::accept(
        &[fo4_scene::hypotheses::LAND_SPACING],
        s.level,
        fo4_scene::hypotheses::status,
    )?;
    let sources = load::open(&s.manifest, &s.data)?;
    let mut out_file = load::output_file(out, Some(&sources.manifest.root))?;
    let options = cells::Options {
        level: s.level,
        terrain: s.terrain,
        walk: true,
        build: load::build_options(s.textures, s.level),
        render: RenderOptions {
            backface_cull: s.backface_cull,
            ..RenderOptions::REFERENCE
        },
        policy: Policy::around(s.around),
        scene_memory: s.scene_memory,
        actors: s.actors,
    };
    let (mut streamer, start) = Streamer::new(&sources, &s.cell, options)?;
    println!(
        "stream: start cell {},{}; {} grid positions with cells in its world, read in {:.2} s; viewer policy: load within {} steps, drop past {}",
        start.grid[0],
        start.grid[1],
        start.world_cells,
        start.seconds,
        options.policy.load,
        options.policy.drop,
    );
    let mut live = Live::default();
    let mut space = WorldSet::new();
    let first = change(&mut streamer, None, &mut live, &mut space)?;
    println!(
        "start: loaded {} cells in {:.2} s worker; credits kept {:.1} MiB (peak {:.1} MiB); {} shared assets; {} collision pieces, {} primitives",
        first.loaded.len(),
        first.worker,
        mib(streamer.credits()),
        mib(streamer.peak()),
        streamer.shared_assets(),
        space.len(),
        space.entries(),
    );
    let start_credits = streamer.credits();

    let p = Params::PROVISIONAL;
    let settings = Settings::DEFAULT;
    let look0 = FlyState::from_eye_look(s.eye, s.look)?;
    let mut view = WalkView::from_fly(look0, p, settings)?;
    let settle = view.character.settle(&space, 600);
    let c = &view.character;
    // The settled start pose: its eye, looking as --eye/--look do (the
    // frame of `--headless --walk` without --path).
    let start_eye = c.eye();
    let start_look: [f64; 3] = std::array::from_fn(|i| start_eye[i] + (s.look[i] - s.eye[i]));
    println!(
        "walk: settled in {settle} ticks, feet {:.3},{:.3},{:.3}, {}",
        c.feet[0],
        c.feet[1],
        c.feet[2],
        if c.grounded { "on ground" } else { "in air" }
    );
    let mut run = Run {
        centre: streamer.centre(),
        streamer,
        live,
        space,
        view,
        heading: look0.yaw,
        ticks: 0,
        airborne: 0,
        crossings: 0,
        most_credits: 0,
    };
    run.most_credits = run.streamer.credits();
    // The settled feet may already be in another cell than the start.
    run.follow()?;
    let mut stalls = 0;
    for (leg, target) in path.iter().enumerate() {
        let before = run.ticks;
        let (outcome, detours) = run.leg(*target)?;
        if outcome != "reached" {
            stalls += 1;
        }
        let c = &run.view.character;
        println!(
            "leg {} to {:.1},{:.1}: {outcome} in {} ticks with {detours} detours, feet {:.1},{:.1},{:.1}, {}",
            leg + 1,
            target[0],
            target[1],
            run.ticks - before,
            c.feet[0],
            c.feet[1],
            c.feet[2],
            if c.grounded { "on ground" } else { "in air" }
        );
    }
    println!(
        "walk: {} legs, {} ticks, {} grid changes, {stalls} legs not reached, {} ticks in the air, {} put back; credits at start {:.1} MiB, at the end {:.1} MiB, most kept {:.1} MiB, peak {:.1} MiB; {:.1} s",
        path.len(),
        run.ticks,
        run.crossings,
        run.airborne,
        run.view.recoveries,
        mib(start_credits),
        mib(run.streamer.credits()),
        mib(run.most_credits),
        mib(run.streamer.peak()),
        started.elapsed().as_secs_f64(),
    );
    let Run {
        streamer,
        mut live,
        space,
        view,
        heading,
        ..
    } = run;
    // The frame from the walker's eye, looking along the last leg with
    // the pitch of --eye/--look; or, with --start-pose, from the settled
    // start pose.
    let (eye, look) = if start_pose {
        (start_eye, start_look)
    } else {
        let eye = view.character.eye();
        let d: [f64; 3] = std::array::from_fn(|i| s.look[i] - s.eye[i]);
        let flat = d[0].hypot(d[1]);
        let look = [
            eye[0] + flat * heading.cos(),
            eye[1] + flat * heading.sin(),
            eye[2] + d[2],
        ];
        (eye, look)
    };
    println!(
        "{} pose: --eye {:.3},{:.3},{:.3} --look {:.3},{:.3},{:.3}",
        if start_pose { "start" } else { "final" },
        eye[0],
        eye[1],
        eye[2],
        look[0],
        look[1],
        look[2]
    );
    let cam =
        fo4_scene::camera::look_at(eye, look, fo4_scene::camera::reference_frustum(size), size)?;
    let mut gpu = live.take_scene()?;
    drop(live);
    drop(space);
    drop(streamer);
    drop(sources);
    let color = crate::gpu::headless(&mut gpu, &cam, RenderOptions::REFERENCE.clear, false)?.color;
    use std::io::Write;
    out_file.write_all(&load::png_rgba8(size, &color)?)?;
    println!("{out}");
    Ok(())
}
