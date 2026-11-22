//! The loading side of the moving grid (`spec/scene.md`, "The moving
//! grid"): one owner of the plugins, the archives and the allocation
//! credits loads the cells the policy asks for, one at a time, and hands
//! each over as plain data ([`Message`]) to whoever draws and walks it:
//! the window's main thread through a channel ([`worker`]), or the
//! headless walk in the same thread.
//!
//! Credits (`--scene-memory`): each loaded cell keeps an account of the
//! root, charged with its walking world and its draw data, released when
//! the cell is dropped; each mesh and texture shared by content keeps its
//! own account, released when the last cell using it is dropped. A cell is
//! read in a child scope of the root (the manifest's `max_memory`), which
//! is released when the cell has been converted. The loader's asset graph
//! is an account of the root that lives while cells are waiting to load
//! and is dropped, with its credits, when none is.
use crate::load::{AnyResult, Sources};
use crate::scene::{GpuMesh, GpuScene, GpuTexture, Instance, Surface};
use crate::stream::{self, AssetKey, Plan, Policy, Shared};
use fo4_core::Budget;
use fo4_plugin::FormKey;
use fo4_render::draw::BuildOptions;
use fo4_render::RenderOptions;
use fo4_scene::{grid::WorldGrid, Loader, RuleLevel};
use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Instant;

/// What one load produced, for the log.
#[derive(Debug, Clone, Default)]
pub struct LoadReport {
    /// `cell`, `no_cell` or `ambiguous_grid` ([`fo4_scene::grid::Claim`]).
    pub outcome: &'static str,
    pub seconds: f64,
    pub placements: usize,
    pub omitted: usize,
    /// `built:<diagonal>`, `refused:<code>` or `not_loaded`.
    pub terrain: String,
    pub triangles: u64,
    /// Meshes and textures sent with their bytes, and bytes sent.
    pub new_assets: usize,
    pub new_bytes: u64,
    /// Meshes and textures already held for another cell.
    pub shared_assets: usize,
    /// Credits of the whole scene once the cell is kept.
    pub credits: u64,
    /// Seconds spent reading the cell and its assets, building its draw
    /// list, its textured terrain, its collision and walking world, and
    /// converting the draw list to GPU data ([`Phases::NAMES`]).
    pub phases: [f64; 5],
    /// The cell's placed actors, when drawn ([`crate::load::actor_summary`]).
    pub actors: Option<String>,
}

/// Names of [`LoadReport::phases`].
pub struct Phases;
impl Phases {
    pub const NAMES: [&'static str; 5] = ["read", "draw", "terrain", "collision", "convert"];
    pub fn line(p: &[f64; 5]) -> String {
        let parts: Vec<String> = Self::NAMES
            .iter()
            .zip(p)
            .map(|(n, s)| format!("{n} {s:.3}"))
            .collect();
        parts.join(", ")
    }
}

/// One cell, ready to draw and walk on.
pub struct CellPiece {
    pub grid: [i32; 2],
    /// Its placed collision and terrain, with `--walk`.
    pub collision: Option<fo4_walk::World>,
    /// Key of each mesh and texture the surfaces and instances index.
    pub meshes: Vec<AssetKey>,
    pub textures: Vec<AssetKey>,
    /// Data of the keys no kept cell held yet; the others are held already.
    pub new_meshes: Vec<(AssetKey, GpuMesh)>,
    pub new_textures: Vec<(AssetKey, GpuTexture)>,
    pub surfaces: Vec<Surface>,
    pub instances: Vec<Instance>,
    pub report: LoadReport,
}

pub enum Message {
    /// The sources are open and the start cell found.
    Ready(Start),
    Load(Box<CellPiece>),
    Drop {
        grid: [i32; 2],
        credits: u64,
    },
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Start {
    pub grid: [i32; 2],
    pub world_cells: usize,
    pub seconds: f64,
}

/// What to load for each cell.
#[derive(Clone, Copy)]
pub struct Options {
    pub level: RuleLevel,
    /// Draw each cell's terrain (`--terrain`).
    pub terrain: bool,
    /// Build each cell's walking world (`--walk`).
    pub walk: bool,
    pub build: BuildOptions,
    pub render: RenderOptions,
    pub policy: Policy,
    /// `--scene-memory`, in bytes.
    pub scene_memory: u64,
    /// Draw each cell's placed actors (`--no-actors` turns it off).
    pub actors: bool,
}

/// What a kept cell holds on the loading side: its credits and the shared
/// keys it uses.
struct Kept {
    _account: Budget,
    keys: Vec<AssetKey>,
}

pub struct Streamer<'a> {
    sources: &'a Sources,
    options: Options,
    grid: WorldGrid,
    _index: Budget,
    root: Budget,
    centre: [i32; 2],
    cells: BTreeMap<[i32; 2], Kept>,
    /// Credits of each shared mesh or texture, by content.
    shared: SharedCredits,
    /// Cells to load, nearest first.
    queue: Vec<[i32; 2]>,
}

impl<'a> Streamer<'a> {
    /// Finds `cell` and every exterior cell of its world by grid, and
    /// plans the cells around it. Fails for a cell that is not an
    /// exterior cell of a world with a grid.
    pub fn new(sources: &'a Sources, cell: &str, options: Options) -> AnyResult<(Self, Start)> {
        let started = Instant::now();
        let root = crate::load::scene_budget(options.scene_memory, &sources.manifest.limits);
        let key = {
            let scope = root.child();
            sources
                .store
                .find_cell(cell, &scope)?
                .ok_or_else(|| format!("MissingCellEditorId: {cell}"))?
        };
        // The index is kept for the whole run, with its credits.
        let index = root.account();
        let grid = fo4_scene::grid::world_grid(&sources.store, &key, &index)?;
        let (Some(at), true) = (grid.grid, grid.searched) else {
            return Err(format!(
                "NotStreamable: {cell} is not an exterior cell of a world with a grid"
            )
            .into());
        };
        let mut s = Self {
            sources,
            options,
            grid,
            _index: index,
            root,
            centre: at,
            cells: BTreeMap::new(),
            shared: SharedCredits::default(),
            queue: Vec::new(),
        };
        s.queue = stream::plan(at, s.cells.keys(), options.policy).load;
        let start = Start {
            grid: at,
            world_cells: s.grid.cells.len(),
            seconds: started.elapsed().as_secs_f64(),
        };
        Ok((s, start))
    }

    /// A loader for the next cells; its asset graph is an account of the
    /// root, released when it is dropped.
    pub fn loader(&self) -> Loader<'a> {
        let s = self.sources;
        Loader::new(
            &s.store,
            &s.vfs,
            &s.manifest.language,
            self.options.level,
            &self.root,
        )
    }

    pub fn centre(&self) -> [i32; 2] {
        self.centre
    }
    /// Whether a cell is waiting to load.
    pub fn pending(&self) -> bool {
        !self.queue.is_empty()
    }
    /// Credits of the scene: kept cells, shared data and, while one
    /// lives, the loader's asset graph.
    pub fn credits(&self) -> u64 {
        self.root.used()
    }
    pub fn peak(&self) -> u64 {
        self.root.measurements().map_or(0, |m| m.peak)
    }
    pub fn kept(&self) -> impl Iterator<Item = &[i32; 2]> {
        self.cells.keys()
    }
    pub fn shared_assets(&self) -> usize {
        self.shared.len()
    }

    /// The player is over `centre`: drops what the policy drops (returned,
    /// in the order they must be applied) and plans the loads again.
    pub fn move_to(&mut self, centre: [i32; 2]) -> Vec<Message> {
        self.centre = centre;
        let Plan { drop, load } = stream::plan(centre, self.cells.keys(), self.options.policy);
        self.queue = load;
        drop.into_iter()
            .map(|grid| {
                self.drop_cell(grid);
                Message::Drop {
                    grid,
                    credits: self.root.used(),
                }
            })
            .collect()
    }

    fn drop_cell(&mut self, grid: [i32; 2]) {
        if let Some(kept) = self.cells.remove(&grid) {
            self.shared.release(&kept.keys);
        }
    }

    /// Loads the nearest waiting cell with `loader`.
    pub fn load_next(&mut self, loader: &mut Loader<'a>) -> AnyResult<Option<Message>> {
        if self.queue.is_empty() {
            return Ok(None);
        }
        let grid = self.queue.remove(0);
        let started = Instant::now();
        let account = self.root.account();
        let (cell, outcome) = self.grid.at(grid);
        let cell = cell.cloned();
        let mut report = LoadReport {
            outcome,
            terrain: "not_loaded".into(),
            ..LoadReport::default()
        };
        let (gpu, collision) = match &cell {
            Some(key) => self.read_cell(loader, key, &account, &mut report)?,
            None => (GpuScene::default(), None),
        };
        let GpuScene {
            meshes,
            textures,
            surfaces,
            instances,
        } = gpu;
        let mut piece = CellPiece {
            grid,
            collision,
            meshes: Vec::new(),
            textures: Vec::new(),
            new_meshes: Vec::new(),
            new_textures: Vec::new(),
            surfaces,
            instances,
            report,
        };
        let mut keys = Vec::new();
        for m in meshes {
            let key = stream::mesh_key(&m);
            piece.meshes.push(key);
            if self.hold(&mut keys, key)? {
                piece.new_meshes.push((key, m));
            }
        }
        for t in textures {
            let key = stream::texture_key(&t);
            piece.textures.push(key);
            if self.hold(&mut keys, key)? {
                piece.new_textures.push((key, t));
            }
        }
        // What the drawing side keeps of the cell apart from shared data.
        let own = std::mem::size_of::<Instance>() * piece.instances.len()
            + std::mem::size_of::<Surface>() * piece.surfaces.len()
            + std::mem::size_of::<AssetKey>() * (piece.meshes.len() + piece.textures.len() * 2);
        account.charge(own as u64, "streamed cell", 0)?;
        let r = &mut piece.report;
        r.new_assets = piece.new_meshes.len() + piece.new_textures.len();
        r.new_bytes = piece.new_meshes.iter().map(|(k, _)| k.bytes).sum::<u64>()
            + piece.new_textures.iter().map(|(k, _)| k.bytes).sum::<u64>();
        r.shared_assets = keys.len() - r.new_assets;
        self.cells.insert(
            grid,
            Kept {
                _account: account,
                keys,
            },
        );
        piece.report.credits = self.root.used();
        piece.report.seconds = started.elapsed().as_secs_f64();
        Ok(Some(Message::Load(Box::new(piece))))
    }

    fn hold(&mut self, keys: &mut Vec<AssetKey>, key: AssetKey) -> AnyResult<bool> {
        Ok(self.shared.hold(&self.root, keys, key)?)
    }

    /// Reads one cell: its draw list (and terrain) as GPU data, and its
    /// walking world charged to `account`.
    fn read_cell(
        &self,
        loader: &mut Loader<'a>,
        key: &FormKey,
        account: &Budget,
        report: &mut LoadReport,
    ) -> AnyResult<(GpuScene, Option<fo4_walk::World>)> {
        let s = self.sources;
        let o = self.options;
        let limits = &s.manifest.limits;
        let work = self.root.child();
        let mut clock = Instant::now();
        let mut lap = |k: usize, report: &mut LoadReport| {
            report.phases[k] += clock.elapsed().as_secs_f64();
            clock = Instant::now();
        };
        let scene = loader.cell(key, &work)?;
        lap(0, report);
        let mut build = if o.actors {
            let actors = fo4_scene::actor::resolve(&scene, loader, &work)?;
            let build = fo4_render::draw::build_with_actors(
                &scene, &actors, loader, limits, o.build, &work,
            )?;
            let mut text = crate::load::actor_summary(&actors, &build);
            for line in crate::load::actor_lines(&actors, &build) {
                text.push_str("\n      ");
                text.push_str(&line);
            }
            report.actors = Some(text);
            build
        } else {
            fo4_render::draw::build(&scene, loader, limits, o.build, &work)?
        };
        lap(1, report);
        report.placements = build.instances.len();
        report.omitted = build.omitted.len();
        let terrain = if o.terrain || o.walk {
            let options = fo4_scene::terrain::Options {
                level: o.level,
                diagonal: None,
            };
            Some(fo4_scene::terrain::load(&s.store, key, options, &work)?)
        } else {
            None
        };
        let built = match &terrain {
            Some(fo4_scene::terrain::Outcome::Built(t)) => {
                report.terrain = format!("built:{}", t.diagonal.name());
                Some(t)
            }
            Some(fo4_scene::terrain::Outcome::Refused(r)) => {
                report.terrain = format!("refused:{}", r.code);
                None
            }
            None => None,
        };
        if let (true, Some(t)) = (o.terrain, built) {
            // Tagged after the cell's placed instances, as for one cell.
            let tag = u32::try_from(build.instances.len())?;
            let mut cache = fo4_render::terrain::TextureCache::new(o.build);
            fo4_render::terrain::append_textured_terrain(
                &mut build.list,
                t,
                &s.store,
                loader,
                &mut cache,
                tag,
                o.level,
                limits,
                &work,
            )?;
        }
        lap(2, report);
        let collision = if o.walk {
            let options = fo4_scene::collision::Options {
                level: o.level,
                body_placement: None,
            };
            let c = fo4_scene::collision::cell(&scene, loader, limits, options, &work)?;
            let placement = c.body_placement.ok_or(
                "BodyPlacementRuleNotAccepted: no body placement level at this rule level",
            )?;
            let mut world = fo4_walk::World::build(&c, placement, account)?;
            if let Some(t) = built {
                world.add_terrain(t, account)?;
            }
            fo4_walk::accept(&world.rules, o.level, fo4_scene::hypotheses::status)?;
            report.triangles = world.stats.triangles;
            Some(world)
        } else {
            None
        };
        lap(3, report);
        let gpu = crate::scene::convert(&mut build.list, &o.render)?;
        lap(4, report);
        drop(build);
        drop(terrain);
        drop(scene);
        drop(work);
        Ok((gpu, collision))
    }
}

/// Credits of the meshes and textures that kept cells share by content:
/// one account of the root per key, charged with its bytes when the first
/// cell uses it and released when the last cell using it is dropped.
#[derive(Default)]
pub struct SharedCredits {
    shared: Shared<AssetKey, Budget>,
}
impl SharedCredits {
    /// One more user of `key` for the cell whose keys are `keys`; true
    /// when no kept cell held it, so its data must be sent and its credits
    /// are charged now. A key the cell already uses is not counted twice.
    pub fn hold(
        &mut self,
        root: &Budget,
        keys: &mut Vec<AssetKey>,
        key: AssetKey,
    ) -> fo4_core::Result<bool> {
        if keys.contains(&key) {
            return Ok(false);
        }
        if self.shared.acquire(&key) {
            keys.push(key);
            return Ok(false);
        }
        let account = root.account();
        account.charge(key.bytes, "streamed shared data", 0)?;
        keys.push(key);
        self.shared.insert(key, account);
        Ok(true)
    }
    /// The cell whose keys are `keys` is dropped.
    pub fn release(&mut self, keys: &[AssetKey]) {
        for k in keys {
            // The last user's release drops the credits.
            drop(self.shared.release(k));
        }
    }
    pub fn len(&self) -> usize {
        self.shared.len()
    }
}

/// Arguments of [`worker`].
pub struct WorkerArgs {
    pub manifest: String,
    pub data: String,
    pub cell: String,
    pub options: Options,
}

/// The window's loading thread: opens the sources, answers
/// [`Message::Ready`] or [`Message::Failed`], then loads what each centre
/// sent on `centres` asks for, nearest first, checking for a newer centre
/// after every cell. Its loader lives while cells wait and is dropped when
/// none does. Returns when either channel closes.
pub fn worker(args: WorkerArgs, centres: Receiver<[i32; 2]>, out: Sender<Message>) {
    let failed = |e: Box<dyn std::error::Error>| Message::Failed(e.to_string());
    let sources = match crate::load::open(&args.manifest, &args.data) {
        Ok(s) => s,
        Err(e) => {
            let _ = out.send(failed(e));
            return;
        }
    };
    let mut s = match Streamer::new(&sources, &args.cell, args.options) {
        Ok((s, start)) => {
            if out.send(Message::Ready(start)).is_err() {
                return;
            }
            s
        }
        Err(e) => {
            let _ = out.send(failed(e));
            return;
        }
    };
    let mut loader: Option<Loader<'_>> = None;
    loop {
        // Wait for a centre when nothing is waiting to load.
        let first = if s.pending() {
            None
        } else {
            loader = None;
            match centres.recv() {
                Ok(c) => Some(c),
                Err(_) => return,
            }
        };
        let mut latest = first;
        loop {
            match centres.try_recv() {
                Ok(c) => latest = Some(c),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
            }
        }
        if let Some(c) = latest.filter(|c| *c != s.centre()) {
            for m in s.move_to(c) {
                if out.send(m).is_err() {
                    return;
                }
            }
        }
        if !s.pending() {
            continue;
        }
        let l = loader.get_or_insert_with(|| s.loader());
        match s.load_next(l) {
            Ok(Some(m)) => {
                if out.send(m).is_err() {
                    return;
                }
            }
            Ok(None) => {}
            Err(e) => {
                let _ = out.send(failed(e));
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Synthetic plugins only: a world of exterior cells written here.
    use super::*;
    use fo4_core::{Game, Limits, Manifest, PluginSpec, TestDir};

    fn sub(sig: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut v = sig.to_vec();
        v.extend((data.len() as u16).to_le_bytes());
        v.extend(data);
        v
    }
    fn record(sig: &[u8; 4], id: u32, flags: u32, data: &[u8]) -> Vec<u8> {
        let mut v = sig.to_vec();
        for n in [data.len() as u32, flags, id, 0] {
            v.extend(n.to_le_bytes());
        }
        v.extend(131u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(data);
        v
    }
    fn group(kind: u32, label: u32, data: &[u8]) -> Vec<u8> {
        let mut v = b"GRUP".to_vec();
        v.extend((data.len() as u32 + 24).to_le_bytes());
        v.extend(label.to_le_bytes());
        v.extend(kind.to_le_bytes());
        v.extend([0; 8]);
        v.extend(data);
        v
    }
    /// An exterior cell at grid (x, y) with one placed reference.
    fn cell(id: u32, x: i32, y: i32, name: &str) -> Vec<u8> {
        let mut c = sub(b"EDID", format!("{name}\0").as_bytes());
        let mut xclc = x.to_le_bytes().to_vec();
        xclc.extend(y.to_le_bytes());
        xclc.extend(0u32.to_le_bytes());
        c.extend(sub(b"XCLC", &xclc));
        let mut out = record(b"CELL", id, 0, &c);
        let mut r = sub(b"NAME", &0x801u32.to_le_bytes());
        let mut tr = Vec::new();
        for f in [1.0f32, 2., 3., 0., 0., 0.] {
            tr.extend(f.to_le_bytes());
        }
        r.extend(sub(b"DATA", &tr));
        let child = record(b"REFR", id + 1, 0, &r);
        out.extend(group(6, id, &group(9, id, &child)));
        out
    }
    /// A world whose cells cover x -4..=4, y -1..=1 except (2, 1); the
    /// start cell at (0, 0); and an interior cell.
    fn sources() -> (TestDir, Sources) {
        let mut cells = Vec::new();
        let mut id = 0x810;
        for y in -1..=1 {
            for x in -4..=4 {
                if [x, y] == [2, 1] {
                    continue;
                }
                let name = if [x, y] == [0, 0] {
                    "Start".to_string()
                } else {
                    format!("Ext{id:x}")
                };
                cells.extend(cell(id, x, y, &name));
                id += 0x10;
            }
        }
        let world = [
            record(b"WRLD", 0x800, 0, &sub(b"EDID", b"World\0")),
            group(1, 0x800, &group(4, 0, &group(5, 0, &cells))),
        ]
        .concat();
        let mut interior = sub(b"EDID", b"Room\0");
        interior.extend(sub(b"DATA", &1u16.to_le_bytes()));
        let mut body = group(
            0,
            u32::from_le_bytes(*b"STAT"),
            &record(b"STAT", 0x801, 0, &[]),
        );
        body.extend(group(0, u32::from_le_bytes(*b"WRLD"), &world));
        body.extend(group(
            0,
            u32::from_le_bytes(*b"CELL"),
            &group(2, 0, &group(3, 0, &record(b"CELL", 0x7f0, 0, &interior))),
        ));
        let mut hedr = 0.95f32.to_le_bytes().to_vec();
        hedr.extend(0u32.to_le_bytes());
        hedr.extend(0x1000u32.to_le_bytes());
        let mut plugin = record(b"TES4", 0, 1, &sub(b"HEDR", &hedr));
        plugin.extend(body);
        let temp = TestDir::new("view-cells");
        let root = temp.join("Data");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("base.esm"), plugin).unwrap();
        let manifest = Manifest {
            game: Game::Fallout4,
            archive_invalidation: None,
            root,
            language: "english".into(),
            plugins: vec![PluginSpec {
                name: "base.esm".into(),
                path: "base.esm".into(),
            }],
            mounts: vec![],
            required_masters: vec![],
            excluded: vec![],
            build: Some("synthetic".into()),
            limits: Limits::default(),
        };
        let b = Budget::new(256 << 20);
        let store = fo4_plugin::typed::Store::open(&manifest, &b).unwrap();
        let vfs = fo4_vfs::Vfs::mount(&manifest, &b).unwrap();
        (
            temp,
            Sources {
                manifest,
                store,
                vfs,
            },
        )
    }
    fn options() -> Options {
        let level = RuleLevel::Hypotheses;
        Options {
            level,
            terrain: true,
            walk: true,
            build: BuildOptions::reference(level),
            render: RenderOptions::REFERENCE,
            policy: Policy::around(1),
            scene_memory: 256 << 20,
            actors: false,
        }
    }
    /// Loads every waiting cell with one loader, dropped afterwards; the
    /// grids loaded, in order, with each outcome.
    fn drain(s: &mut Streamer<'_>) -> Vec<([i32; 2], &'static str)> {
        let mut loader = s.loader();
        let mut out = Vec::new();
        while let Some(m) = s.load_next(&mut loader).unwrap() {
            let Message::Load(p) = m else { panic!() };
            out.push((p.grid, p.report.outcome));
        }
        out
    }
    fn dropped(ms: Vec<Message>) -> Vec<[i32; 2]> {
        ms.into_iter()
            .map(|m| match m {
                Message::Drop { grid, .. } => grid,
                _ => panic!(),
            })
            .collect()
    }

    #[test]
    fn crossings_load_the_new_column_drop_the_far_one_and_release_its_credits() {
        let (_t, src) = sources();
        let (mut s, start) = Streamer::new(&src, "Start", options()).unwrap();
        assert_eq!(start.grid, [0, 0]);
        assert_eq!(start.world_cells, 26);
        let empty = s.credits();
        let first = drain(&mut s);
        assert_eq!(first.len(), 9);
        assert_eq!(first[0], ([0, 0], "cell"));
        let nine = s.credits();
        assert!(nine > empty);
        // East: the column at x = 2 loads, (2, 1) has no cell.
        assert!(s.move_to([1, 0]).is_empty());
        let east = drain(&mut s);
        assert_eq!(
            east,
            vec![([2, 0], "cell"), ([2, -1], "cell"), ([2, 1], "no_cell")]
        );
        let twelve = s.credits();
        let set_a: Vec<[i32; 2]> = s.kept().copied().collect();
        // Further east: x = -1 drops and its credits go.
        assert_eq!(dropped(s.move_to([2, 0])), vec![[-1, -1], [-1, 0], [-1, 1]]);
        assert!(s.credits() < twelve);
        drain(&mut s);
        // Back to (0, 0): x = 3 drops, x = -1 loads again.
        assert!(dropped(s.move_to([1, 0])).is_empty());
        assert!(drain(&mut s).is_empty());
        assert_eq!(dropped(s.move_to([0, 0])), vec![[3, -1], [3, 0], [3, 1]]);
        drain(&mut s);
        // The same cells kept as after the first step: the same credits.
        assert_eq!(s.kept().copied().collect::<Vec<_>>(), set_a);
        assert_eq!(s.credits(), twelve);
        // Far away and back to nothing kept but the start's nine.
        for x in [1, 2, 3, 4] {
            s.move_to([x, 0]);
            drain(&mut s);
        }
        for x in [3, 2, 1, 0, -1] {
            s.move_to([x, 0]);
            drain(&mut s);
        }
        s.move_to([0, 0]);
        drain(&mut s);
        let kept: Vec<[i32; 2]> = s.kept().copied().collect();
        assert_eq!(kept.len(), 12);
        assert!(kept.iter().all(|g| (-2..=1).contains(&g[0])));
    }

    #[test]
    fn an_interior_cell_is_not_streamed() {
        let (_t, src) = sources();
        let e = Streamer::new(&src, "Room", options()).err().unwrap();
        assert!(e.to_string().starts_with("NotStreamable"), "{e}");
    }

    fn key(n: u8, bytes: u64) -> AssetKey {
        AssetKey {
            kind: 1,
            bytes,
            hash: [u64::from(n); 2],
        }
    }

    #[test]
    fn shared_credits_stay_while_any_cell_uses_them() {
        let root = Budget::new(1 << 20);
        let mut shared = SharedCredits::default();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        assert!(shared.hold(&root, &mut a, key(1, 1000)).unwrap());
        assert!(shared.hold(&root, &mut a, key(2, 300)).unwrap());
        // Twice in one cell: counted once.
        assert!(!shared.hold(&root, &mut a, key(2, 300)).unwrap());
        assert_eq!(a.len(), 2);
        assert_eq!(root.used(), 1300);
        // A second cell shares key 1: no new credits, nothing to send.
        assert!(!shared.hold(&root, &mut b, key(1, 1000)).unwrap());
        assert_eq!(root.used(), 1300);
        shared.release(&a);
        assert_eq!(root.used(), 1000);
        assert_eq!(shared.len(), 1);
        shared.release(&b);
        assert_eq!(root.used(), 0);
        // Over the root's ceiling: refused, nothing held.
        let mut c = Vec::new();
        assert!(shared.hold(&root, &mut c, key(3, 2 << 20)).is_err());
        assert!(c.is_empty() && shared.len() == 0 && root.used() == 0);
    }
}
