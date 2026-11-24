//! Command-line parsing, as pure functions.
use crate::fly::FlySettings;
use fo4_scene::RuleLevel;

pub const USAGE: &str = "\
usage:
  fo4-view MANIFEST DATA_DIRECTORY CELL_EDITOR_ID [--strict|--measured|--under-hypotheses]
           --eye X,Y,Z --look X,Y,Z [--backface-cull] [--terrain] [--around N]
           [--speed UNITS_PER_SECOND] [--fast-speed UNITS_PER_SECOND] [--radians-per-pixel R]
           [--walk] [--texture-cap N] [--alpha-tested-cap N] [--reference-textures]
           [--scene-memory MIB] [--no-stream] [--no-actors]
           [--actor-clip ARCHIVE_PATH.hkx [--actor-clip-time SECONDS]] [--time SECONDS]
           [--clock-emissive-multiple] [--clock-emissive-colour] [--clock-falloff]
           [--clock-translation] [--clock-rotation] [--clock-rotation-xyz] [--clock-scale]
           [--frame-log build/FRAMES.tsv] [--script-path X,Y;... [--script-shots build/NAME]]
           [--play-run SWITCHES_FILE --play-out DIR [--play-reach UNITS] [--play-pick-bounds]
            [--play-live-world [--play-nowait]]
            [--play-input INPUT_FILE [play_run's play options...]
             [--play-shots DIR --play-shot-every SECONDS]
             [--timeline-cast [the cast readings of a --timeline frame]]
             [the light switches of --headless, but --exposure-eye]]]
      Window with a free-fly camera: W/A/S/D move, Shift moves faster, hold the
      left mouse button and move the mouse to look, R returns to the start pose.
      The title bar shows the mode, the speed and the position.
      --walk starts in walk mode instead: the fo4-walk controller on the
      cell's placed collision (needs --under-hypotheses; every movement
      number is a provisional rule), camera at its eye height, Shift runs;
      Tab switches between walking and flying. In an exterior cell the walker
      also stands on the terrain of the cell and its neighbours when the level
      accepts the terrain rules. A walker that stays in the air below the
      lowest collision for a second is put back where it last stood.
      --terrain also draws the cell's own terrain with its ground textures,
      as the CPU render's --terrain (needs --under-hypotheses while its rules
      are pending); weight of a texture no record names or that is refused
      is drawn plain grey.
      --around N (1 to 3) also loads the exterior cells within N grid steps:
      their placed objects are drawn and, with --walk, collided; with
      --terrain their terrain is drawn too. With --walk --terrain --around N
      in an exterior cell the loaded grid follows the player (walking: the
      feet; flying: the eye): a second thread loads the cells within N
      steps of the cell under the player, nearest first, and the cells more
      than N + 1 steps away are dropped (a viewer policy); a walker over a
      cell that has not arrived yet waits in place. --no-stream loads the
      grid once at start instead.
      Block-compressed textures are uploaded as their stored blocks, from
      the first stored level whose sides are at most --texture-cap (default
      1024), or --alpha-tested-cap (default 2048) for a texture drawn with an
      alpha test; other textures are decoded as the CPU render decodes them.
      --reference-textures decodes every texture as the CPU render does
      (first level at most 256 texels), for comparing frames with it.
      --scene-memory MIB (default 2048) bounds the allocation credits of
      everything the loaded scene keeps (records, draw list, stored texture
      blocks, terrain, collision, walking world); the manifest's max_memory
      still bounds each cell read and each asset decode on its own. Credits
      are conservative, not resident memory.
      Placed actors (ACHR) are drawn standing in their skeleton's rest
      pose, CPU-skinned once at load, when the level accepts their rules
      (pending: --under-hypotheses); each actor's report line says what was
      drawn or why not. --no-actors leaves them out, as before.
      --actor-clip names a Havok animation clip by its archive path (a .hkx
      file; spline-compressed clips only):
      every drawn actor whose skeleton's .hkx beside its skeleton NIF the
      clip's binding fits is posed by it (pending rules: --under-hypotheses),
      starting at --actor-clip-time (default 0) and looping, CPU-skinned
      again every frame; the clip is the one named here, not the one the
      game would play. Not with the moving grid (use --no-stream).
      --time starts the game clock at SECONDS after load and advances it
      with the window's frame clock: free-running controllers of shader UV
      fields and alpha are drawn at its time under the pending clock rules
      (--under-hypotheses; the CPU render's --time); without it such models
      are left out, as before. Not with the moving grid (use --no-stream).
      --clock-emissive-multiple, --clock-emissive-colour and --clock-falloff
      (off by default) let the clock drive those effect fields too, each
      under its own pending rule (the CPU render's flags of the same names).
      --clock-translation, --clock-rotation, --clock-rotation-xyz and
      --clock-scale (off by default) let free-running transform controllers
      move their nodes' drawn geometry on the clock (each channel and
      rotation key form under its own pending rule; collision stays where
      it is stored).
      Measurement aids: --frame-log writes one line per frame (interval,
      time in the frame's update, cells drawn and uploading, entities).
      --script-path (with --walk) steers the walker through the waypoints
      as the headless --path does once the cells around the start are
      drawn, then returns to the start pose as R does and closes the
      window when its cells are drawn again; --script-shots captures the
      window at the start pose before the walk (NAME-start.png) and after
      the return (NAME-return.png), from the same camera pose.
      --play-run (with --walk --under-hypotheses; a viewer built with
      --features script-event; not with the moving grid) plays the opening:
      quest_run new-game runs on its own thread with the switches the file
      lists (one per line, --track FILE among them: its first point is the
      start, where the walker is put), the walker's feet each frame and the
      frame clock drive it, E activates what the eye's ray meets within
      --play-reach units (the caller's number), and the title bar shows the
      run's second, the last stage and the line being spoken. The run's
      outputs (--timeline among its switches) go to --play-out when the
      window closes (spec/play.md). With --play-live-world the walking
      world and the drawn references follow the run's state each frame
      (disabled references hidden and passed through, initially disabled
      ones the run enables shown, moved ones moved; --play-nowait reads the
      NoWait forms as the plain natives).
      --play-input FILE drives the run with play_run's walker and that
      scripted input instead of the keyboard and mouse, with every play_run
      play option (--walk-speed, --play-slice, --play-load-doors,
      --play-follow-grid, --play-elevator..., --play-player-moveto, ...;
      without --play-input only the four above are taken). The run waits
      for the window's clock; the camera stands at the walker's eye, and
      when the walker's walking world is built about another cell (a load
      door, a MoveTo, a sit, a grid square left) the window loads and draws
      that cell, the run waiting meanwhile. --play-shots DIR
      --play-shot-every SECONDS writes the window's frame as a PNG into DIR
      (under ./build, as every output) each time the run passes a multiple
      of SECONDS; the window closes when the run ends. With --timeline-cast
      (and the readings a --timeline frame takes with it: the start clip
      flags, --timeline-child-flag-agrees, --timeline-dead-*, the look
      readings...) the run's actors are drawn as their characters from the
      run's own timeline as it grows (the cell's placed actors are not):
      each time the run changes an actor's place or presence (or a death,
      a sit) the cast of the drawn cell is built again on its own thread,
      at the run's second then, and replaces the one drawn. The light
      switches of --headless (all but --exposure-eye) light the cells the
      window loads and draw the sky, as the headless frame does.
  fo4-view MANIFEST DATA_DIRECTORY CELL_EDITOR_ID [level] --headless
           --eye X,Y,Z --look X,Y,Z --out build/FRAME.png [--tags build/TAGS.bin]
           [--size WxH] [--backface-cull] [--terrain] [--around N] [--walk]
           [--texture-cap N] [--alpha-tested-cap N] [--reference-textures]
           [--scene-memory MIB] [--path X,Y;X,Y;... [--start-pose]] [--no-actors]
           [--actor-clip ARCHIVE_PATH.hkx [--actor-clip-time SECONDS]] [--time SECONDS]
           [--script-event INDEX:EVENT[,EVENT...]]
      One offscreen frame, no window. With --actor-clip the actors it fits
      are drawn posed at --actor-clip-time (not with --path). With --time
      the free-running controllers are drawn at that time of the game clock
      (not with --path). --tags also writes the per-pixel instance
      tags (u32 little-endian, u32::MAX = background). With --walk the walking
      world is built as in the window, the walker is dropped with its eye at
      --eye and settled, and the frame is drawn from its eye looking the
      same way; the settled pose is printed.
      --script-event (a viewer built with --features script-event; needs
      --under-hypotheses; not with --walk) runs the
      scripts of the cell's placed reference INDEX (numbered as
      `script_run CELL list` numbers them) for the events named (activate,
      trigger-enter, trigger-leave, load, unload, init, reset, cell-attach,
      cell-detach, fragment:N) with fo4-vm's data host, prints the
      effect log, leaves out of the frame each reference of the loaded
      cells whose last visibility change in the log is a disable, and puts
      in each initially disabled one whose last change is an enable; every
      other native call is only logged.
      --path (with --walk --terrain --around N) walks the settled walker to
      each X,Y in turn at the run speed, moving the loaded grid as the
      window does but loading synchronously, prints every grid change
      (cells dropped and loaded, time per cell, credits kept) and draws
      the frame from where the walk ends, looking along its last leg;
      with --start-pose, from the settled start pose instead (the frame
      without --path), with the cells kept when the walk ends.
  fo4-view MANIFEST DATA_DIRECTORY CELL_EDITOR_ID|- --under-hypotheses --headless
           --timeline FILE --at SECONDS --eye-height UNITS [--look-heading DEGREES]
           [--timeline-nowait] [--timeline-child-flag-agrees]
           [--timeline-dead-lying | --timeline-dead-left-out] [--timeline-sit-marker]
           [--timeline-cast [--timeline-target-rotation] [--actor-missing-bone-bind]
           [--actor-start-clip] [--actor-subgraph-keywords] [--actor-clip-folder]
           [--actor-start-clip-loop] [--actor-sit-clip] [--actor-subgraph-flag-one]
           [--actor-model-graph-idle] [--actor-template-bits] [--actor-leveled-seed SEED
           [--actor-leveled-chance-none] [--actor-leveled-use-all]]
           [--timeline-ride-model PLUGIN:HEX [--timeline-ride SEQUENCE,START]]
           [--skinned-models-own-nodes]] [--memory-report FILE]
           [--look-at X,Y,Z | --look-at-actor nearest|speaker]
           [--overlay build/OVERLAY.txt] --out build/FRAME.png
           [--tags build/TAGS.bin] [--size WxH] [--terrain] [--around N] [...]
      One frame of a quest run's timeline (quest_run new-game --timeline,
      spec/timeline.md) at second SECONDS of the run, as the CPU render's
      --timeline draws it: CELL - loads the cell the player's track is in;
      the references the run disabled are left out, those it enabled put
      in, those it moved moved (pending rules); placed actors are drawn as
      placeholder markers where the run puts them, not skinned; the camera
      is EYE_HEIGHT units above the track's position, looking level along
      its movement or along --look-heading (degrees counter-clockwise from
      +X); the overlay (stages, objectives, the line being spoken, what was
      applied) is printed and written to --overlay. --timeline-nowait also
      applies the NoWait forms of Enable and Disable (a pending rule);
      --timeline-child-flag-agrees decides actors whose enable chain is
      open only for their own initially-disabled flag where it agrees with
      the chain (a pending rule). --timeline-cast draws the actors as their
      skinned characters in the skeleton's rest pose at the run's place
      (stored rotation; --timeline-target-rotation: the rotation of the one
      reference at a moved actor's place; pending rules), a marker only
      where an actor cannot be drawn, with the reason in the overlay;
      --actor-missing-bone-bind skins a bone the skeleton lacks at its bind
      transform instead of leaving the actor out (a pending rule).
      --actor-start-clip poses each character by the clip its race's
      behaviour graph reaches from its root (start states, stored initial
      values, highest weights), at the clip generator's start time; it
      needs --actor-subgraph-keywords (the race's subgraph entry the actor's
      keywords fit) and --actor-clip-folder (the clip file in the entry's
      folders) to reach a file; --actor-start-clip-loop poses at the
      frame's second, looping where the generator's mode says so (pending
      rules, spec/character-graph.md); an actor refused keeps the rest pose,
      with the reason in the overlay. The run's death and sit lines
      (spec/timeline.md, Deaths and furniture): --timeline-dead-lying draws
      a dead actor lying at its last place (a placeholder),
      --timeline-dead-left-out leaves it out; --timeline-sit-marker puts a
      sitting actor at its furniture's marker nearest it, facing the
      marker's heading, and --actor-sit-clip poses it by the clip its race's
      subgraph entry for the furniture's keywords reaches (pending rules).
      A templated or leveled actor (spec/actor.md, Picked looks):
      --actor-template-bits takes its traits, outfit and keywords through
      the template slots of bits 0, 8 and 12; --actor-leveled-seed SEED
      picks one entry of each leveled list from the caller's seed (a
      caller assumption for the game's choice; pending rules);
      --actor-leveled-chance-none reads a leveled item list's LVLD as a
      chance of nothing rolled from the seed, --actor-leveled-use-all gives
      every entry of a list whose LVLF has bit 2 (pending rules).
      --actor-subgraph-flag-one lets a race without subgraph entries of flag
      word 0 take its entries of flag word 1 (no target keywords);
      --actor-model-graph-idle draws an actor's part model whose own graph's
      start state plays no sequence as stored (pending rules,
      spec/character-graph.md and spec/actor.md).
      --timeline-ride-model draws the named reference by its skinned shapes
      on its own nodes; --timeline-ride SEQUENCE,START puts the nodes that
      sequence moves where it has them at the frame's second, the sequence
      starting at the run's second START (the caller's; pending rules).
      --skinned-models-own-nodes draws every placed reference the draw
      refuses for a skinned shape by its skinned shapes on its own nodes
      (a pending rule; rigid shapes, controllers and material swap left out).
      --memory-report FILE writes where the frame's texture and mesh bytes
      go (measurement only).
      --look-at and --look-at-actor aim the camera from the same eye at a
      point, the nearest drawn actor or the line's speaker (presentation).
      Not with --eye, --look, --walk, --path, --actor-clip or --script-event.
  fo4-view ... --headless [--light-cell] [--light-inherits] [--light-points
           [--light-reference-radius] [--light-fade]] [--light-fog]
           [--sky-weather] [--light-weather] [--fog-weather]
           [--assume-hour H] [--assume-weather N]
      Light and sky (headless, with or without --timeline; spec/light.md),
      each a pending rule off by default, as the CPU render draws them: an
      interior cell's ambient and directional colour (--light-cell), the
      fields its Inherits word names from its lighting template
      (--light-inherits), placed lights as points with a linear falloff to
      their radius (--light-points; --light-reference-radius adds the
      reference's XRDS, --light-fade multiplies by the base's fade value),
      the cell's fog (--light-fog); outside, the climate's weather at the
      caller's hour (--assume-hour; --assume-weather N where the climate
      lists several): the sky as a gradient of its colour rows
      (--sky-weather), its directional ambient as an ambient cube
      (--light-weather) and its fog (--fog-weather). Effect surfaces and
      markers stay unlit.
  fo4-view compare --cpu CPU.png --gpu GPU.png --diff build/DIFF.png
           [--cpu-tags CPU.bin --gpu-tags GPU.bin]
      Coverage agreement and colour differences of two frames of one pose.";

#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub manifest: String,
    pub data: String,
    pub cell: String,
    pub level: RuleLevel,
    pub eye: [f64; 3],
    pub look: [f64; 3],
    pub backface_cull: bool,
    /// Draw the cell's terrain (`--terrain`), as the CPU render does.
    pub terrain: bool,
    /// Also load the exterior cells within this many grid steps
    /// (`--around`, 0 = the cell alone), as the CPU render does.
    pub around: u32,
    pub textures: Textures,
    /// Credits the loaded scene may keep, in bytes (`--scene-memory`).
    pub scene_memory: u64,
    /// Draw the placed actors (default; `--no-actors` turns it off).
    pub actors: bool,
    /// Pose the drawn actors with a clip (`--actor-clip`,
    /// `--actor-clip-time`).
    pub clip: Option<crate::clip::Options>,
    /// Run a placed reference's scripts before drawing (`--script-event`).
    pub script_event: Option<crate::vm_event::ScriptEvent>,
    /// Seconds of the game clock after load (`--time`): the build's time
    /// (`fo4_render::draw::BuildOptions::time`), advanced in the window.
    pub time: Option<f64>,
    /// The clock's switches (`--clock-emissive-multiple`,
    /// `--clock-emissive-colour`, `--clock-falloff`; all off by default).
    pub clock: fo4_scene::clock::Switches,
    /// A run timeline's frame (`--timeline`, headless only); `eye` and
    /// `look` are then unused.
    pub timeline: Option<crate::timeline::Args>,
    /// The light step's switches (`--light-cell` and the others, headless
    /// only; all off by default) and the caller's hour and weather
    /// (`--assume-hour`, `--assume-weather`).
    pub light: fo4_scene::light::Switches,
    pub light_assumptions: fo4_scene::light::Assumptions,
    /// The window's scripted play mode with `--timeline-cast` (no
    /// `--timeline`): the run's actors drawn as their characters from the
    /// run's own timeline as it grows, with the headless frame's cast
    /// readings (the same structure; its file, second, eye height and
    /// camera fields are not read). The cell's own placed actors are then
    /// not drawn.
    pub play_cast: Option<crate::timeline::Args>,
}

/// Default of `--scene-memory`: a viewer bound, not a claim about the game.
/// About 1.2 GiB of credits are kept at `--around 3` at the test cell
/// (README), with a peak resident memory near 2 GB.
pub const DEFAULT_SCENE_MEMORY_MIB: u64 = 2048;

/// `--scene-memory` in MiB, within the manifest's own absolute ceiling of
/// one account (16 GiB).
fn scene_memory(text: &str) -> Result<u64, String> {
    text.parse::<u64>()
        .ok()
        .filter(|v| (64..=16384).contains(v))
        .map(|v| v << 20)
        .ok_or_else(|| format!("ExpectedSceneMemoryMiB 64 to 16384: {text}"))
}

/// How the viewer keeps its textures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Textures {
    /// Decoded as the CPU reference decodes them (`--reference-textures`).
    Reference,
    /// Block-compressed textures kept as stored blocks, from the first level
    /// whose sides are at most `cap`, or `alpha_tested_cap` for a texture
    /// drawn with an alpha test (`--texture-cap`, `--alpha-tested-cap`).
    Blocks { cap: u32, alpha_tested_cap: u32 },
}

/// Texture defaults: chosen by measured memory and look (README), with no
/// claim about the game's texture streaming.
pub const DEFAULT_TEXTURES: Textures = Textures::Blocks {
    cap: 1024,
    alpha_tested_cap: 2048,
};

fn texture_side(text: &str) -> Result<u32, String> {
    text.parse::<u32>()
        .ok()
        .filter(|v| (4..=16384).contains(v))
        .ok_or_else(|| format!("ExpectedTextureSide 4 to 16384: {text}"))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Window {
        scene: Scene,
        fly: FlySettings,
        /// Start in walk mode (`walk`), with Tab to switch.
        walk: bool,
        /// Move the loaded grid with the player (`--walk --terrain
        /// --around N` without `--no-stream`).
        stream: bool,
        /// Measurement aids.
        debug: WindowDebug,
        /// The play mode (`--play-run`): the walker drives a quest run.
        play: Option<Play>,
    },
    Headless {
        scene: Scene,
        size: [u32; 2],
        out: String,
        tags: Option<String>,
        /// Settle a walker from the eye and draw from its eye.
        walk: bool,
        /// Waypoints walked with the moving grid (`--path`).
        path: Option<Vec<[f64; 2]>>,
        /// Draw the `--path` frame from the settled start pose
        /// (`--start-pose`).
        start_pose: bool,
    },
    Compare {
        cpu: String,
        gpu: String,
        diff: String,
        tags: Option<(String, String)>,
    },
}

/// The window's play mode (`--play-run FILE --play-out DIR`, with
/// `--walk`; `spec/play.md`): the walker's feet each frame and the
/// activation key (E) drive `quest_run new-game` on its own thread.
#[derive(Debug, Clone, PartialEq)]
pub struct Play {
    /// `quest_run new-game`'s switches, one per line (`--track FILE`
    /// among them: its first point is the start).
    pub run: String,
    /// Where the run writes its outputs when the window closes.
    pub out: String,
    /// `play_run`'s play options (`crate::play_switches`). With the
    /// keyboard only the activation's reach (`--play-reach`, the caller's
    /// number; without it the key does nothing) and object bounds
    /// (`--play-pick-bounds`, `play_activate_object_bounds_v1`) and the
    /// live world (`--play-live-world`,
    /// `play_live_collision_follows_run_v1`; `--play-nowait`) apply.
    pub switches: Box<crate::play_switches::Switches>,
    /// `--play-input FILE`: the run driven by `play_run`'s walker and this
    /// scripted input instead of the keyboard and mouse.
    pub input: Option<String>,
    /// `--play-shots DIR --play-shot-every SECONDS`: the window's frame
    /// written each time the scripted run passes a multiple of SECONDS.
    pub shots: Option<(String, f64)>,
}

/// Measurement aids of the window (`--frame-log`, `--script-path`,
/// `--script-shots`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WindowDebug {
    pub frame_log: Option<String>,
    pub path: Option<Vec<[f64; 2]>>,
    pub shots: Option<String>,
}

/// Free-fly defaults: plain viewer settings with no claim about the game.
pub const DEFAULT_FLY: FlySettings = FlySettings {
    speed: 300.,
    fast_speed: 1500.,
    radians_per_pixel: 0.003,
};

pub fn point(text: &str) -> Result<[f64; 3], String> {
    let mut values = text.split(',').map(str::parse::<f64>);
    let mut out = [0.; 3];
    for v in &mut out {
        *v = values
            .next()
            .ok_or("ExpectedThreeCoordinates")?
            .map_err(|_| "ExpectedThreeCoordinates")?;
    }
    if values.next().is_some() || out.iter().any(|v| !v.is_finite()) {
        return Err("ExpectedThreeCoordinates".into());
    }
    Ok(out)
}

pub fn size(text: &str) -> Result<[u32; 2], String> {
    let (w, h) = text.split_once('x').ok_or("ExpectedWidthxHeight")?;
    let parse = |s: &str| {
        s.parse::<u32>()
            .ok()
            .filter(|v| (1..=16384).contains(v))
            .ok_or("ExpectedWidthxHeight")
    };
    Ok([parse(w)?, parse(h)?])
}

fn radius(text: &str) -> Result<u32, String> {
    text.parse::<u32>()
        .ok()
        .filter(|n| (1..=fo4_scene::grid::MAX_RADIUS).contains(n))
        .ok_or_else(|| {
            format!(
                "ExpectedGridRadius 1 to {}: {text}",
                fo4_scene::grid::MAX_RADIUS
            )
        })
}

/// Most waypoints of `--path`.
pub const MAX_WAYPOINTS: usize = 256;

/// `X,Y;X,Y;...`: finite world coordinates.
pub fn waypoints(text: &str) -> Result<Vec<[f64; 2]>, String> {
    let bad = || format!("ExpectedWaypoints X,Y;X,Y (at most {MAX_WAYPOINTS}): {text}");
    let mut out = Vec::new();
    for part in text.split(';') {
        let mut v = part.split(',').map(str::parse::<f64>);
        let (Some(Ok(x)), Some(Ok(y)), None) = (v.next(), v.next(), v.next()) else {
            return Err(bad());
        };
        if !(x.is_finite() && y.is_finite()) || out.len() == MAX_WAYPOINTS {
            return Err(bad());
        }
        out.push([x, y]);
    }
    Ok(out)
}

fn not_negative(text: &str) -> Result<f64, String> {
    text.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.)
        .ok_or_else(|| format!("ExpectedSecondsNotNegative: {text}"))
}

pub(crate) fn finite(text: &str) -> Result<f64, String> {
    text.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("ExpectedFiniteNumber: {text}"))
}

fn positive(text: &str) -> Result<f64, String> {
    text.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.)
        .ok_or_else(|| format!("ExpectedPositiveNumber: {text}"))
}

pub fn parse(args: &[String]) -> Result<Command, String> {
    let usage = || USAGE.to_string();
    if args.first().map(String::as_str) == Some("compare") {
        let mut cpu = None;
        let mut gpu = None;
        let mut diff = None;
        let mut cpu_tags = None;
        let mut gpu_tags = None;
        let mut rest = args[1..].iter();
        while let Some(arg) = rest.next() {
            let slot = match arg.as_str() {
                "--cpu" => &mut cpu,
                "--gpu" => &mut gpu,
                "--diff" => &mut diff,
                "--cpu-tags" => &mut cpu_tags,
                "--gpu-tags" => &mut gpu_tags,
                _ => return Err(usage()),
            };
            *slot = Some(rest.next().ok_or_else(usage)?.clone());
        }
        let tags = match (cpu_tags, gpu_tags) {
            (Some(a), Some(b)) => Some((a, b)),
            (None, None) => None,
            _ => return Err(usage()),
        };
        return Ok(Command::Compare {
            cpu: cpu.ok_or_else(usage)?,
            gpu: gpu.ok_or_else(usage)?,
            diff: diff.ok_or_else(usage)?,
            tags,
        });
    }
    if args.len() < 3 {
        return Err(usage());
    }
    let mut level = RuleLevel::Measured;
    let mut eye = None;
    let mut look = None;
    let mut backface_cull = false;
    let mut headless = false;
    let mut out = None;
    let mut tags = None;
    let mut frame = fo4_scene::camera::REFERENCE_SIZE;
    let mut fly = DEFAULT_FLY;
    let mut walk = false;
    let mut terrain = false;
    let mut around = 0;
    let mut reference_textures = false;
    let mut cap = None;
    let mut alpha_tested_cap = None;
    let mut memory = DEFAULT_SCENE_MEMORY_MIB << 20;
    let mut path = None;
    let mut no_stream = false;
    let mut start_pose = false;
    let mut actors = true;
    let mut clip_path = None;
    let mut clip_time = None;
    let mut debug = WindowDebug::default();
    let mut script_event = None;
    let mut time = None;
    let mut clock = fo4_scene::clock::Switches::default();
    let mut timeline_file = None;
    let mut timeline_at = None;
    let mut eye_height = None;
    let mut look_heading = None;
    let mut timeline_nowait = false;
    let mut overlay = None;
    let mut timeline_cast = false;
    let mut target_rotation = false;
    let mut missing_bones_at_bind = false;
    let mut pose = fo4_scene::char_graph::Switches::default();
    let mut child_flag_agrees = false;
    let mut dead = fo4_scene::timeline::Dead::Standing;
    let mut sit_marker = false;
    let mut look_readings = fo4_scene::actor::Readings::default();
    let mut model_graph_idle = false;
    let mut skinned_models = false;
    let mut memory_report: Option<String> = None;
    let mut ride_model = None;
    let mut ride_sequence = None;
    let mut aim = fo4_scene::timeline::Aim::Track;
    let mut light = fo4_scene::light::Switches::default();
    let mut light_assumptions = fo4_scene::light::Assumptions::default();
    let (mut play_run, mut play_out) = (None, None);
    let mut play_switches = crate::play_switches::Switches::default();
    let (mut play_input, mut play_shots, mut play_shot_every) = (None, None, None);
    let mut rest = args[3..].iter();
    while let Some(arg) = rest.next() {
        if clock.set_flag(arg) || light.set_flag(arg) || pose.set_flag(arg) {
            continue;
        }
        // `play_run`'s play options, read by the one shared structure.
        let here = &args[args.len() - rest.len() - 1..];
        let taken = play_switches.take(here)?;
        if taken > 0 {
            if taken > 1 {
                rest.nth(taken - 2);
            }
            continue;
        }
        let mut value = || rest.next().ok_or_else(usage);
        match arg.as_str() {
            "--strict" => level = RuleLevel::Facts,
            "--measured" => level = RuleLevel::Measured,
            "--under-hypotheses" => level = RuleLevel::Hypotheses,
            "--eye" => eye = Some(point(value()?)?),
            "--look" => look = Some(point(value()?)?),
            "--backface-cull" => backface_cull = true,
            "--headless" => headless = true,
            "--out" => out = Some(value()?.clone()),
            "--tags" => tags = Some(value()?.clone()),
            "--size" => frame = size(value()?)?,
            "--speed" => fly.speed = positive(value()?)?,
            "--fast-speed" => fly.fast_speed = positive(value()?)?,
            "--radians-per-pixel" => fly.radians_per_pixel = positive(value()?)?,
            "--walk" => walk = true,
            "--terrain" => terrain = true,
            "--around" => around = radius(value()?)?,
            "--reference-textures" => reference_textures = true,
            "--texture-cap" => cap = Some(texture_side(value()?)?),
            "--alpha-tested-cap" => alpha_tested_cap = Some(texture_side(value()?)?),
            "--scene-memory" => memory = scene_memory(value()?)?,
            "--path" => path = Some(waypoints(value()?)?),
            "--no-stream" => no_stream = true,
            "--start-pose" => start_pose = true,
            "--no-actors" => actors = false,
            "--actor-clip" => clip_path = Some(value()?.clone()),
            "--actor-clip-time" => clip_time = Some(not_negative(value()?)?),
            "--time" => time = Some(not_negative(value()?)?),
            "--frame-log" => debug.frame_log = Some(value()?.clone()),
            "--script-path" => debug.path = Some(waypoints(value()?)?),
            "--script-shots" => debug.shots = Some(value()?.clone()),
            "--script-event" => script_event = Some(crate::vm_event::parse(value()?)?),
            "--play-run" => play_run = Some(value()?.clone()),
            "--play-out" => play_out = Some(value()?.clone()),
            "--play-input" => play_input = Some(value()?.clone()),
            "--play-shots" => play_shots = Some(value()?.clone()),
            "--play-shot-every" => play_shot_every = Some(positive(value()?)?),
            "--timeline" => timeline_file = Some(value()?.clone()),
            "--at" => timeline_at = Some(not_negative(value()?)?),
            "--eye-height" => eye_height = Some(finite(value()?)?),
            "--look-heading" => look_heading = Some(finite(value()?)?),
            "--timeline-nowait" => timeline_nowait = true,
            "--overlay" => overlay = Some(value()?.clone()),
            "--timeline-cast" => timeline_cast = true,
            "--timeline-target-rotation" => target_rotation = true,
            "--actor-missing-bone-bind" => missing_bones_at_bind = true,
            "--timeline-child-flag-agrees" => child_flag_agrees = true,
            "--timeline-dead-lying" | "--timeline-dead-left-out" => {
                if dead != fo4_scene::timeline::Dead::Standing {
                    return Err("one of --timeline-dead-lying and --timeline-dead-left-out".into());
                }
                dead = if arg == "--timeline-dead-lying" {
                    fo4_scene::timeline::Dead::Lying
                } else {
                    fo4_scene::timeline::Dead::LeftOut
                };
            }
            "--timeline-sit-marker" => sit_marker = true,
            "--actor-template-bits" => look_readings.template_bits = true,
            "--actor-model-graph-idle" => model_graph_idle = true,
            "--skinned-models-own-nodes" => skinned_models = true,
            "--memory-report" => memory_report = Some(value()?.to_string()),
            "--actor-leveled-chance-none" => look_readings.chance_none = true,
            "--actor-leveled-use-all" => look_readings.use_all = true,
            "--timeline-ride-model" => {
                ride_model = Some(fo4_scene::timeline::parse_key(value()?).ok_or_else(usage)?)
            }
            "--timeline-ride" => {
                let (k, s) = value()?.split_once(',').ok_or_else(usage)?;
                ride_sequence = Some((k.parse::<usize>().map_err(|_| usage())?, finite(s)?));
            }
            "--actor-leveled-seed" => {
                look_readings.leveled_seed = Some(value()?.parse::<u64>().map_err(|_| usage())?)
            }
            "--assume-hour" => {
                light_assumptions.hour = Some(
                    finite(value()?)
                        .ok()
                        .filter(|h| (0. ..=24.).contains(h))
                        .ok_or_else(usage)?,
                )
            }
            "--assume-weather" => {
                light_assumptions.weather = Some(value()?.parse::<usize>().map_err(|_| usage())?)
            }
            "--look-at" => aim = fo4_scene::timeline::Aim::Point(point(value()?)?),
            "--look-at-actor" => {
                aim = match value()?.as_str() {
                    "nearest" => fo4_scene::timeline::Aim::Nearest,
                    "speaker" => fo4_scene::timeline::Aim::Speaker,
                    _ => return Err(usage()),
                }
            }
            _ => return Err(usage()),
        }
    }
    let textures = match (reference_textures, DEFAULT_TEXTURES) {
        (true, _) if cap.is_some() || alpha_tested_cap.is_some() => {
            return Err("--reference-textures takes no texture cap".into())
        }
        (true, _) => Textures::Reference,
        (
            false,
            Textures::Blocks {
                cap: c,
                alpha_tested_cap: a,
            },
        ) => Textures::Blocks {
            cap: cap.unwrap_or(c),
            alpha_tested_cap: alpha_tested_cap.unwrap_or(a),
        },
        (false, Textures::Reference) => Textures::Reference,
    };
    // The window's scripted play mode: the cast readings with the run's own
    // timeline (`--play-run ... --play-input`, `--timeline-cast`, no
    // `--timeline`, `--at` or `--eye-height`, no frame-only option).
    let play_cast_asked = play_run.is_some()
        && play_input.is_some()
        && timeline_cast
        && timeline_file.is_none()
        && timeline_at.is_none()
        && eye_height.is_none()
        && look_heading.is_none()
        && overlay.is_none()
        && memory_report.is_none()
        && ride_model.is_none()
        && ride_sequence.is_none()
        && aim == fo4_scene::timeline::Aim::Track;
    let play_cast = play_cast_asked.then(|| crate::timeline::Args {
        file: String::new(),
        at: 0.,
        eye_height: 0.,
        heading: None,
        nowait: timeline_nowait,
        overlay: None,
        child_flag_agrees,
        cast: true,
        target_rotation,
        missing_bones_at_bind,
        pose,
        aim: fo4_scene::timeline::Aim::Track,
        dead,
        sit_marker,
        look: look_readings,
        model_graph_idle,
        skinned_models,
        memory_report: None,
        ride: None,
    });
    let timeline = match (timeline_file, timeline_at, eye_height) {
        _ if play_cast.is_some() => None,
        (Some(file), Some(at), Some(eye_height)) => Some(crate::timeline::Args {
            file,
            at,
            eye_height,
            heading: look_heading,
            nowait: timeline_nowait,
            overlay,
            child_flag_agrees,
            cast: timeline_cast,
            target_rotation,
            missing_bones_at_bind,
            pose,
            aim,
            dead,
            sit_marker,
            look: look_readings,
            model_graph_idle,
            skinned_models,
            memory_report: memory_report.clone(),
            ride: ride_model.clone().map(|reference| fo4_scene::timeline::ride::Options {
                reference,
                sequence: ride_sequence,
            }),
        }),
        (None, None, None)
            if look_heading.is_none()
                && !timeline_nowait
                && overlay.is_none()
                && !timeline_cast
                && !target_rotation
                && !missing_bones_at_bind
                && !pose.any()
                && !child_flag_agrees
                && dead == fo4_scene::timeline::Dead::Standing
                && !sit_marker
                && !look_readings.any()
                && !model_graph_idle
                && !skinned_models
                && memory_report.is_none()
                && ride_model.is_none()
                && ride_sequence.is_none()
                && aim == fo4_scene::timeline::Aim::Track =>
        {
            None
        }
        _ => {
            return Err(
                "--timeline needs --at and --eye-height; --look-heading, --timeline-nowait, --timeline-child-flag-agrees, --timeline-dead-lying, --timeline-dead-left-out, --timeline-sit-marker, --timeline-cast, --timeline-target-rotation, --actor-missing-bone-bind, the start clip flags, --look-at, --look-at-actor and --overlay need --timeline"
                    .into(),
            )
        }
    };
    if timeline.is_some() {
        if eye.is_some() || look.is_some() {
            return Err("--timeline places the camera: no --eye or --look".into());
        }
        if !headless || walk || path.is_some() || script_event.is_some() || clip_path.is_some() {
            return Err(
                "--timeline applies to --headless without --walk, --path, --script-event or --actor-clip"
                    .into(),
            );
        }
        // Actors are drawn as the timeline's markers, or with
        // --timeline-cast as its characters: never the cell's own.
        actors = false;
        if (ride_sequence.is_some() && ride_model.is_none())
            || ((look_readings.chance_none || look_readings.use_all)
                && look_readings.leveled_seed.is_none())
        {
            return Err(
                "--timeline-ride needs --timeline-ride-model; --actor-leveled-chance-none and --actor-leveled-use-all need --actor-leveled-seed"
                    .into(),
            );
        }
        if (target_rotation
            || missing_bones_at_bind
            || pose.any()
            || look_readings.any()
            || model_graph_idle
            || skinned_models
            || ride_model.is_some())
            && !timeline_cast
        {
            return Err(
                "--timeline-target-rotation, --actor-missing-bone-bind and the start clip flags need --timeline-cast"
                    .into(),
            );
        }
    } else if args[2] == "-" {
        return Err("CELL - (the player's cell) needs --timeline".into());
    }
    if play_cast.is_some() {
        // The run's actors are the cast: never the cell's own.
        actors = false;
        if (look_readings.chance_none || look_readings.use_all)
            && look_readings.leveled_seed.is_none()
        {
            return Err(
                "--actor-leveled-chance-none and --actor-leveled-use-all need --actor-leveled-seed"
                    .into(),
            );
        }
    }
    let unused = timeline.as_ref().map(|_| [0.; 3]);
    let scene = Scene {
        manifest: args[0].clone(),
        data: args[1].clone(),
        cell: args[2].clone(),
        level,
        eye: eye.or(unused).ok_or_else(usage)?,
        look: look.or(unused).ok_or_else(usage)?,
        backface_cull,
        terrain,
        around,
        textures,
        scene_memory: memory,
        actors,
        clip: match (clip_path, clip_time) {
            (Some(path), time) => Some(crate::clip::Options {
                path,
                time: time.unwrap_or(0.),
            }),
            (None, None) => None,
            (None, Some(_)) => return Err("--actor-clip-time needs --actor-clip".into()),
        },
        script_event,
        time,
        clock,
        timeline,
        light,
        light_assumptions,
        play_cast,
    };
    // The window's scripted play mode takes the light switches too (no
    // `--exposure-eye`: the window's camera moves).
    let play_window = !headless && play_run.is_some() && play_input.is_some();
    if (light.any() || light_assumptions != fo4_scene::light::Assumptions::default())
        && (!(headless || play_window)
            || headless && walk
            || scene.script_event.is_some()
            || play_window && light.exposure_eye)
    {
        return Err(
            "the light switches apply to --headless without --walk or --script-event, or to the window's --play-run with --play-input (not --exposure-eye)".into(),
        );
    }
    if (light.reference_radius || light.fade) && !light.points
        || light.inherits && !(light.cell || light.fog)
        || light_assumptions != fo4_scene::light::Assumptions::default()
            && !(light.weather() || light.image_space())
    {
        return Err("--light-reference-radius and --light-fade need --light-points, --light-inherits needs --light-cell or --light-fog, --assume-hour and --assume-weather need a weather switch".into());
    }
    if scene.script_event.is_some() && (!headless || walk) {
        return Err("--script-event applies to --headless without --walk".into());
    }
    if scene.clip.is_some() && !scene.actors {
        return Err("--actor-clip poses the drawn actors: not with --no-actors".into());
    }
    let grid_moves = walk && scene.terrain && scene.around > 0;
    let shots = match (play_shots, play_shot_every) {
        (Some(dir), Some(every)) => Some((dir, every)),
        (None, None) => None,
        _ => return Err("--play-shots DIR and --play-shot-every SECONDS go together".into()),
    };
    let play = match (play_run, play_out) {
        (Some(run), Some(out)) => Some(Play {
            run,
            out,
            switches: Box::new(play_switches),
            input: play_input,
            shots,
        }),
        (None, None) if !play_switches.any() && play_input.is_none() && shots.is_none() => None,
        _ => {
            return Err(
                "--play-run FILE needs --play-out DIR; --play-reach, --play-pick-bounds, --play-live-world, --play-nowait, --play-input, --play-shots and play_run's other play options need --play-run"
                    .into(),
            )
        }
    };
    if let Some(p) = &play {
        p.switches.check()?;
        if p.input.is_none() && p.switches.walker_only() {
            return Err("PlayNeedsInput: play_run's walker options (all but --play-reach, --play-pick-bounds, --play-live-world and --play-nowait) drive the scripted walker: give --play-input FILE".into());
        }
        if p.input.is_none() && p.shots.is_some() {
            return Err("--play-shots needs --play-input".into());
        }
    }
    if play.is_some()
        && (headless
            || !walk
            || level != RuleLevel::Hypotheses
            || grid_moves && !no_stream
            || debug.path.is_some()
            || scene.script_event.is_some())
    {
        return Err("--play-run applies to the window with --walk and --under-hypotheses, not with the moving grid (add --no-stream), --script-path or --script-event".into());
    }
    if debug.shots.is_some() && debug.path.is_none() {
        return Err("--script-shots needs --script-path".into());
    }
    if debug.path.is_some() && !walk {
        return Err("--script-path needs --walk".into());
    }
    if headless {
        if debug != WindowDebug::default() {
            return Err("--frame-log and --script-path apply to the window only".into());
        }
        if no_stream {
            return Err("--no-stream applies to the window only".into());
        }
        if path.is_some() && !grid_moves {
            return Err("--path needs --walk --terrain --around N".into());
        }
        if path.is_some() && tags.is_some() {
            return Err("--path draws no instance tags".into());
        }
        if start_pose && path.is_none() {
            return Err("--start-pose applies to --path only".into());
        }
        if path.is_some() && scene.clip.is_some() {
            return Err("--actor-clip does not apply to --path".into());
        }
        if path.is_some() && scene.time.is_some() {
            return Err("--time does not apply to --path".into());
        }
        Ok(Command::Headless {
            scene,
            size: frame,
            out: out.ok_or_else(usage)?,
            tags,
            walk,
            path,
            start_pose,
        })
    } else if out.is_some()
        || tags.is_some()
        || path.is_some()
        || start_pose
        || frame != fo4_scene::camera::REFERENCE_SIZE
    {
        Err("--out, --tags, --size, --path and --start-pose apply to --headless only".into())
    } else if grid_moves && !no_stream && scene.clip.is_some() {
        Err("--actor-clip does not apply to the moving grid: add --no-stream".into())
    } else if grid_moves && !no_stream && scene.time.is_some() {
        Err("--time does not apply to the moving grid: add --no-stream".into())
    } else {
        Ok(Command::Window {
            scene,
            fly,
            walk,
            stream: grid_moves && !no_stream,
            debug,
            play,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn headless_reference_command() {
        let c = parse(&args(
            "m.json data Financial13 --under-hypotheses --headless --eye 0,153.6,104.49 \
             --look 70.7,224.3,104.49 --out build/view/a.png --tags build/view/a.bin",
        ))
        .unwrap();
        let Command::Headless {
            scene,
            size,
            out,
            tags,
            walk,
            path,
            start_pose,
        } = c
        else {
            panic!("{c:?}")
        };
        assert!(path.is_none());
        assert!(!start_pose);
        assert_eq!(scene.level, RuleLevel::Hypotheses);
        assert_eq!(scene.eye, [0., 153.6, 104.49]);
        assert_eq!(scene.look, [70.7, 224.3, 104.49]);
        assert!(!scene.backface_cull);
        assert_eq!(size, [1280, 720]);
        assert_eq!(out, "build/view/a.png");
        assert_eq!(tags.as_deref(), Some("build/view/a.bin"));
        assert!(!walk);
        assert!(scene.actors, "placed actors are drawn by default");
    }

    #[test]
    fn actor_clip_options() {
        let c = parse(&args(
            "m d C --headless --eye 1,2,3 --look 4,5,6 --out a.png --actor-clip a/b.hkx --actor-clip-time 1.5",
        ))
        .unwrap();
        let Command::Headless { scene, .. } = c else {
            panic!("{c:?}")
        };
        assert_eq!(
            scene.clip,
            Some(crate::clip::Options {
                path: "a/b.hkx".into(),
                time: 1.5
            })
        );
        let Command::Window { scene, .. } =
            parse(&args("m d C --eye 1,2,3 --look 4,5,6 --actor-clip a.hkx")).unwrap()
        else {
            panic!()
        };
        assert_eq!(scene.clip.map(|c| c.time), Some(0.));
        let c = parse(&args(
            "m d C --headless --eye 1,2,3 --look 4,5,6 --out a.png --time 2 --clock-falloff --clock-rotation-xyz",
        ))
        .unwrap();
        let Command::Headless { scene, .. } = c else {
            panic!("{c:?}")
        };
        assert_eq!(
            scene.clock.flags(),
            ["--clock-falloff", "--clock-rotation-xyz"]
        );
        assert!(scene.clock.rotation_xyz && !scene.clock.rotation);
        for (line, error) in [
            ("m d C --eye 1,2,3 --look 4,5,6 --actor-clip-time 1", "--actor-clip-time needs --actor-clip"),
            ("m d C --eye 1,2,3 --look 4,5,6 --actor-clip a.hkx --no-actors", "--actor-clip poses the drawn actors: not with --no-actors"),
            ("m d C --eye 1,2,3 --look 4,5,6 --actor-clip a.hkx --actor-clip-time -1", "ExpectedSecondsNotNegative: -1"),
            ("m d C --under-hypotheses --eye 1,2,3 --look 4,5,6 --walk --terrain --around 1 --actor-clip a.hkx", "--actor-clip does not apply to the moving grid: add --no-stream"),
            ("m d C --headless --eye 1,2,3 --look 4,5,6 --out a.png --walk --terrain --around 1 --path 1,2 --actor-clip a.hkx", "--actor-clip does not apply to --path"),
        ] {
            assert_eq!(parse(&args(line)).unwrap_err(), error, "{line}");
        }
        assert!(parse(&args(
            "m d C --under-hypotheses --eye 1,2,3 --look 4,5,6 --walk --terrain --around 1 --no-stream --actor-clip a.hkx"
        ))
        .is_ok());
    }

    #[cfg(feature = "script-event")]
    #[test]
    fn script_event_is_headless_only_and_not_with_walk() {
        let c = parse(&args(
            "m d C --under-hypotheses --headless --eye 1,2,3 --look 4,5,6 --out a.png --script-event 7:activate",
        ))
        .unwrap();
        let Command::Headless { scene, .. } = c else {
            panic!("{c:?}")
        };
        assert_eq!(
            scene.script_event,
            Some(crate::vm_event::ScriptEvent {
                index: 7,
                events: vec!["activate".into()]
            })
        );
        for line in [
            "m d C --under-hypotheses --eye 1,2,3 --look 4,5,6 --script-event 7:activate",
            "m d C --under-hypotheses --headless --eye 1,2,3 --look 4,5,6 --out a.png --walk --script-event 7:activate",
            "m d C --under-hypotheses --headless --eye 1,2,3 --look 4,5,6 --out a.png --script-event 7:dance",
        ] {
            assert!(parse(&args(line)).is_err(), "{line}");
        }
    }

    #[cfg(not(feature = "script-event"))]
    #[test]
    fn script_event_is_refused_without_the_feature() {
        let line = "m d C --under-hypotheses --headless --eye 1,2,3 --look 4,5,6 --out a.png --script-event 7:activate";
        assert!(parse(&args(line))
            .unwrap_err()
            .contains("--features script-event"));
    }

    #[test]
    fn a_timeline_frame_takes_its_camera_and_cell_from_the_run() {
        let c = parse(&args(
            "m d - --under-hypotheses --headless --timeline build/t.tsv --at 12.5 \
             --eye-height 120 --out build/a.png --overlay build/a.txt",
        ))
        .unwrap();
        let Command::Headless { scene, .. } = c else {
            panic!("{c:?}")
        };
        let t = scene.timeline.unwrap();
        assert_eq!((t.at, t.eye_height, t.heading), (12.5, 120., None));
        assert_eq!(t.overlay.as_deref(), Some("build/a.txt"));
        assert!(!t.nowait);
        assert!(!t.cast && !t.target_rotation && !t.child_flag_agrees);
        assert_eq!(t.aim, fo4_scene::timeline::Aim::Track);
        assert!(!scene.actors, "actors are the timeline's markers");
        let c = parse(&args(
            "m d - --under-hypotheses --headless --timeline build/t.tsv --at 1 --eye-height 120 \
             --out build/a.png --timeline-cast --timeline-target-rotation \
             --timeline-child-flag-agrees --look-at-actor speaker --actor-missing-bone-bind",
        ))
        .unwrap();
        let Command::Headless { scene, .. } = c else {
            panic!("{c:?}")
        };
        let t = scene.timeline.unwrap();
        assert!(t.cast && t.target_rotation && t.child_flag_agrees && t.missing_bones_at_bind);
        assert_eq!(t.pose, fo4_scene::char_graph::Switches::default());
        assert_eq!(t.aim, fo4_scene::timeline::Aim::Speaker);
        let c = parse(&args(
            "m d - --under-hypotheses --headless --timeline build/t.tsv --at 1 --eye-height 120 \
             --out build/a.png --timeline-cast --actor-start-clip --actor-subgraph-keywords \
             --actor-clip-folder --actor-start-clip-loop",
        ))
        .unwrap();
        let Command::Headless { scene, .. } = c else {
            panic!("{c:?}")
        };
        let p = scene.timeline.unwrap().pose;
        assert!(p.start && p.subgraph && p.folder && p.cycle);
        assert!(!scene.actors, "the cast is the timeline's, not the cell's");
        for line in [
            "m d - --under-hypotheses --headless --out a.png",
            "m d C --under-hypotheses --headless --timeline t --at 1 --out a.png",
            "m d C --under-hypotheses --headless --timeline t --at 1 --eye-height 1 --eye 1,2,3 --look 4,5,6 --out a.png",
            "m d C --under-hypotheses --timeline t --at 1 --eye-height 1",
            "m d C --under-hypotheses --headless --timeline t --at 1 --eye-height 1 --walk --out a.png",
            "m d C --under-hypotheses --headless --eye 1,2,3 --look 4,5,6 --out a.png --look-heading 3",
            "m d C --under-hypotheses --headless --eye 1,2,3 --look 4,5,6 --out a.png --timeline-cast",
            "m d C --under-hypotheses --headless --eye 1,2,3 --look 4,5,6 --out a.png --look-at-actor nearest",
            "m d - --under-hypotheses --headless --timeline t --at 1 --eye-height 1 --out a.png --timeline-target-rotation",
            "m d - --under-hypotheses --headless --timeline t --at 1 --eye-height 1 --out a.png --actor-missing-bone-bind",
            "m d - --under-hypotheses --headless --timeline t --at 1 --eye-height 1 --out a.png --actor-start-clip",
            "m d C --under-hypotheses --headless --eye 1,2,3 --look 4,5,6 --out a.png --actor-clip-folder",
            "m d - --under-hypotheses --headless --timeline t --at 1 --eye-height 1 --out a.png --look-at-actor far",
        ] {
            assert!(parse(&args(line)).is_err(), "{line}");
        }
    }

    #[test]
    fn no_actors_turns_the_actors_off_in_both_modes() {
        for line in [
            "m d C --eye 1,2,3 --look 4,5,6 --no-actors",
            "m d C --headless --eye 1,2,3 --look 4,5,6 --out a.png --no-actors",
        ] {
            let scene = match parse(&args(line)).unwrap() {
                Command::Window { scene, .. } | Command::Headless { scene, .. } => scene,
                c => panic!("{c:?}"),
            };
            assert!(!scene.actors, "{line}");
        }
    }

    #[test]
    fn play_needs_the_walker_and_its_outputs() {
        let ok = parse(&args(
            "m d C --under-hypotheses --walk --eye 1,2,3 --look 4,5,6 --play-run s.txt --play-out o --play-reach 150 --play-pick-bounds",
        ))
        .unwrap();
        let Command::Window { play: Some(p), .. } = ok else {
            panic!("{ok:?}")
        };
        assert_eq!(
            p,
            Play {
                run: "s.txt".into(),
                out: "o".into(),
                switches: Box::new(crate::play_switches::Switches {
                    reach: Some(150.0),
                    pick_bounds: true,
                    ..Default::default()
                }),
                input: None,
                shots: None,
            }
        );
        let live = parse(&args(
            "m d C --under-hypotheses --walk --eye 1,2,3 --look 4,5,6 --play-run s.txt --play-out o --play-live-world --play-nowait",
        ))
        .unwrap();
        let Command::Window { play: Some(p), .. } = live else {
            panic!("{live:?}")
        };
        assert!(p.switches.live_world && p.switches.nowait);
        // The scripted input takes every play option of play_run.
        let scripted = parse(&args(
            "m d C --under-hypotheses --walk --eye 1,2,3 --look 4,5,6 --play-run s.txt --play-out o --play-input i.txt --walk-speed 300 --play-reach 150 --play-load-doors --play-follow-grid --play-slice 0.25 --play-elevator a.esm:00000001 --play-elevator-rigid --play-shots d --play-shot-every 30",
        ))
        .unwrap();
        let Command::Window { play: Some(p), .. } = scripted else {
            panic!("{scripted:?}")
        };
        assert_eq!(p.input.as_deref(), Some("i.txt"));
        assert_eq!(p.shots, Some(("d".to_string(), 30.0)));
        assert_eq!(p.switches.slice, Some(0.25));
        assert_eq!(p.switches.elevators, ["a.esm:00000001"]);
        assert!(p.switches.load_doors && p.switches.follow_grid && p.switches.rigid);
        for bad in [
            "m d C --under-hypotheses --walk --eye 1,2,3 --look 4,5,6 --play-run s.txt",
            "m d C --under-hypotheses --eye 1,2,3 --look 4,5,6 --play-run s.txt --play-out o",
            "m d C --walk --eye 1,2,3 --look 4,5,6 --play-run s.txt --play-out o",
            "m d C --under-hypotheses --walk --headless --out a.png --eye 1,2,3 --look 4,5,6 --play-run s.txt --play-out o",
            "m d C --under-hypotheses --walk --terrain --around 1 --eye 1,2,3 --look 4,5,6 --play-run s.txt --play-out o",
            "m d C --under-hypotheses --walk --eye 1,2,3 --look 4,5,6 --play-reach 10",
            "m d C --under-hypotheses --walk --eye 1,2,3 --look 4,5,6 --play-run s.txt --play-out o --play-reach -1",
            "m d C --under-hypotheses --walk --eye 1,2,3 --look 4,5,6 --play-live-world",
            "m d C --under-hypotheses --walk --eye 1,2,3 --look 4,5,6 --play-run s.txt --play-out o --play-nowait",
            // Walker options without the scripted input, shots without it.
            "m d C --under-hypotheses --walk --eye 1,2,3 --look 4,5,6 --play-run s.txt --play-out o --play-load-doors",
            "m d C --under-hypotheses --walk --eye 1,2,3 --look 4,5,6 --play-run s.txt --play-out o --play-shots d --play-shot-every 1",
            "m d C --under-hypotheses --walk --eye 1,2,3 --look 4,5,6 --play-run s.txt --play-out o --play-input i --play-shots d",
            "m d C --under-hypotheses --walk --eye 1,2,3 --look 4,5,6 --play-input i",
            "m d C --under-hypotheses --walk --eye 1,2,3 --look 4,5,6 --play-run s.txt --play-out o --play-input i --play-elevator-rigid",
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad}");
        }
        let c = parse(&args(
            "m d C --under-hypotheses --walk --terrain --around 1 --no-stream --eye 1,2,3 --look 4,5,6 --play-run s.txt --play-out o",
        ))
        .unwrap();
        assert!(matches!(
            c,
            Command::Window {
                play: Some(_),
                stream: false,
                ..
            }
        ));
    }

    #[test]
    fn the_scripted_play_window_takes_the_cast_and_light_readings() {
        let base = "m d C --under-hypotheses --walk --eye 1,2,3 --look 4,5,6 --play-run s.txt --play-out o --play-input i.txt";
        let c = parse(&args(&format!(
            "{base} --timeline-cast --actor-start-clip --actor-subgraph-keywords --actor-clip-folder --timeline-child-flag-agrees --light-cell --sky-weather --assume-hour 12"
        )))
        .unwrap();
        let Command::Window { scene, .. } = c else {
            panic!("{c:?}")
        };
        let cast = scene.play_cast.unwrap();
        assert!(cast.cast && cast.child_flag_agrees && cast.pose.start && cast.pose.folder);
        assert!(scene.timeline.is_none());
        assert!(!scene.actors, "the run's actors, not the cell's");
        assert!(scene.light.cell && scene.light.sky);
        assert_eq!(scene.light_assumptions.hour, Some(12.));
        // Default: none of it.
        let Command::Window { scene, .. } = parse(&args(base)).unwrap() else {
            panic!()
        };
        assert!(scene.play_cast.is_none() && scene.actors && !scene.light.any());
        for bad in [
            // The cast readings need --timeline-cast (and the scripted run).
            format!("{base} --actor-start-clip"),
            "m d C --under-hypotheses --walk --eye 1,2,3 --look 4,5,6 --play-run s.txt --play-out o --timeline-cast".to_string(),
            // Frame-only options stay with --timeline.
            format!("{base} --timeline-cast --look-at-actor nearest"),
            // The window's camera moves: no exposure by the eye.
            format!("{base} --light-cell --exposure-eye"),
            // Without --play-run the light switches stay headless.
            "m d C --eye 1,2,3 --look 4,5,6 --light-cell".to_string(),
        ] {
            assert!(parse(&args(&bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn window_defaults_and_options() {
        let c = parse(&args("m d C --eye 1,2,3 --look 4,5,6")).unwrap();
        assert_eq!(
            c,
            Command::Window {
                scene: Scene {
                    manifest: "m".into(),
                    data: "d".into(),
                    cell: "C".into(),
                    level: RuleLevel::Measured,
                    eye: [1., 2., 3.],
                    look: [4., 5., 6.],
                    backface_cull: false,
                    terrain: false,
                    around: 0,
                    textures: DEFAULT_TEXTURES,
                    scene_memory: 2048 << 20,
                    actors: true,
                    clip: None,
                    script_event: None,
                    time: None,
                    clock: fo4_scene::clock::Switches::default(),
                    timeline: None,
                    light: fo4_scene::light::Switches::default(),
                    light_assumptions: fo4_scene::light::Assumptions::default(),
                    play_cast: None,
                },
                fly: DEFAULT_FLY,
                walk: false,
                stream: false,
                debug: WindowDebug::default(),
                play: None,
            }
        );
        let c = parse(&args(
            "m d C --strict --eye 1,2,3 --look 4,5,6 --speed 50 --fast-speed 75",
        ))
        .unwrap();
        let Command::Window {
            scene, fly, walk, ..
        } = c
        else {
            panic!()
        };
        assert_eq!(scene.level, RuleLevel::Facts);
        assert_eq!((fly.speed, fly.fast_speed), (50., 75.));
        assert!(!walk);
        let c = parse(&args(
            "m d C --under-hypotheses --eye 1,2,3 --look 4,5,6 --walk",
        ))
        .unwrap();
        assert!(matches!(c, Command::Window { walk: true, .. }));
        let c = parse(&args(
            "m d C --under-hypotheses --eye 1,2,3 --look 4,5,6 --walk --terrain",
        ))
        .unwrap();
        assert!(matches!(&c, Command::Window { walk: true, scene, .. } if scene.terrain));
        let c = parse(&args(
            "m d C --under-hypotheses --headless --eye 1,2,3 --look 4,5,6 --out a.png --terrain",
        ))
        .unwrap();
        assert!(matches!(&c, Command::Headless { scene, .. } if scene.terrain));
        let c = parse(&args(
            "m d C --under-hypotheses --headless --eye 1,2,3 --look 4,5,6 --out a.png --around 2",
        ))
        .unwrap();
        assert!(matches!(&c, Command::Headless { scene, walk: false, .. } if scene.around == 2));
        let c = parse(&args(
            "m d C --under-hypotheses --headless --eye 1,2,3 --look 4,5,6 --out a.png --walk",
        ))
        .unwrap();
        assert!(matches!(c, Command::Headless { walk: true, .. }));
        let c = parse(&args("m d C --eye 1,2,3 --look 4,5,6 --walk --around 1")).unwrap();
        assert!(
            matches!(&c, Command::Window { walk: true, scene, stream: false, .. } if scene.around == 1)
        );
        let c = parse(&args(
            "m d C --eye 1,2,3 --look 4,5,6 --walk --terrain --around 2",
        ))
        .unwrap();
        assert!(matches!(&c, Command::Window { stream: true, .. }));
        let c = parse(&args(
            "m d C --eye 1,2,3 --look 4,5,6 --walk --terrain --around 2 --no-stream",
        ))
        .unwrap();
        assert!(matches!(&c, Command::Window { stream: false, .. }));
        let c = parse(&args(
            "m d C --headless --eye 1,2,3 --look 4,5,6 --out a.png --walk --terrain --around 1 --path 1,2;-3.5,4e3",
        ))
        .unwrap();
        assert!(
            matches!(&c, Command::Headless { path: Some(p), start_pose: false, .. } if p == &[[1., 2.], [-3.5, 4000.]])
        );
        let c = parse(&args(
            "m d C --headless --eye 1,2,3 --look 4,5,6 --out a.png --walk --terrain --around 1 --path 1,2 --start-pose",
        ))
        .unwrap();
        assert!(matches!(
            &c,
            Command::Headless {
                path: Some(_),
                start_pose: true,
                ..
            }
        ));
        let c = parse(&args(
            "m d C --eye 1,2,3 --look 4,5,6 --walk --terrain --around 2 --frame-log build/f.tsv --script-path 1,2;3,4 --script-shots build/s",
        ))
        .unwrap();
        assert!(
            matches!(&c, Command::Window { stream: true, debug, .. } if *debug == WindowDebug {
                frame_log: Some("build/f.tsv".into()),
                path: Some(vec![[1., 2.], [3., 4.]]),
                shots: Some("build/s".into()),
            })
        );
        let c = parse(&args("m d C --eye 1,2,3 --look 4,5,6 --texture-cap 512")).unwrap();
        assert!(matches!(&c, Command::Window { scene, .. } if scene.textures
            == Textures::Blocks { cap: 512, alpha_tested_cap: 2048 }));
        let c = parse(&args("m d C --eye 1,2,3 --look 4,5,6 --scene-memory 1024")).unwrap();
        assert!(matches!(&c, Command::Window { scene, .. } if scene.scene_memory == 1 << 30));
        let c = parse(&args("m d C --eye 1,2,3 --look 4,5,6 --reference-textures")).unwrap();
        assert!(
            matches!(&c, Command::Window { scene, .. } if scene.textures == Textures::Reference)
        );
    }

    #[test]
    fn refusals() {
        for bad in [
            "m d C --eye 1,2,3",
            "m d C --eye 1,2 --look 1,2,3",
            "m d C --eye 1,2,3 --look 1,2,3,4",
            "m d C --eye 1,2,3 --look 4,5,6 --out x.png",
            "m d C --headless --eye 1,2,3 --look 4,5,6",
            "m d C --eye 1,2,3 --look 4,5,6 --speed -1",
            "m d C --eye 1,2,3 --look 4,5,6 --bogus",
            "m d C --headless --eye 1,2,3 --look 4,5,6 --out a.png --size 0x5",
            "m d C --eye 1,2,3 --look 4,5,6 --around 0",
            "m d C --eye 1,2,3 --look 4,5,6 --around 4",
            "m d C --eye 1,2,3 --look 4,5,6 --around x",
            "m d C --eye 1,2,3 --look 4,5,6 --around",
            "m d C --eye 1,2,3 --look 4,5,6 --texture-cap 2",
            "m d C --eye 1,2,3 --look 4,5,6 --reference-textures --texture-cap 512",
            "m d C --eye 1,2,3 --look 4,5,6 --scene-memory 63",
            "m d C --eye 1,2,3 --look 4,5,6 --scene-memory 16385",
            "m d C --eye 1,2,3 --look 4,5,6 --scene-memory x",
            "m d C --eye 1,2,3 --look 4,5,6 --walk --terrain --around 1 --path 1,2",
            "m d C --headless --eye 1,2,3 --look 4,5,6 --out a.png --walk --around 1 --path 1,2",
            "m d C --headless --eye 1,2,3 --look 4,5,6 --out a.png --walk --terrain --around 1 --path 1,2 --tags t.bin",
            "m d C --headless --eye 1,2,3 --look 4,5,6 --out a.png --walk --terrain --around 1 --path 1,2;3",
            "m d C --headless --eye 1,2,3 --look 4,5,6 --out a.png --walk --terrain --around 1 --path 1,x",
            "m d C --headless --eye 1,2,3 --look 4,5,6 --out a.png --no-stream",
            "m d C --headless --eye 1,2,3 --look 4,5,6 --out a.png --walk --terrain --around 1 --start-pose",
            "m d C --eye 1,2,3 --look 4,5,6 --walk --terrain --around 1 --start-pose",
            "m d C --eye 1,2,3 --look 4,5,6 --script-path 1,2",
            "m d C --eye 1,2,3 --look 4,5,6 --walk --script-shots build/s",
            "m d C --headless --eye 1,2,3 --look 4,5,6 --out a.png --frame-log build/f.tsv",
            "compare --cpu a --gpu b",
            "compare --cpu a --gpu b --diff c --cpu-tags x",
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad}");
        }
        assert_eq!(size("640x360").unwrap(), [640, 360]);
        let c = parse(&args(
            "compare --cpu a.png --gpu b.png --diff c.png --cpu-tags a.bin --gpu-tags b.bin",
        ))
        .unwrap();
        assert_eq!(
            c,
            Command::Compare {
                cpu: "a.png".into(),
                gpu: "b.png".into(),
                diff: "c.png".into(),
                tags: Some(("a.bin".into(), "b.bin".into())),
            }
        );
    }
}
