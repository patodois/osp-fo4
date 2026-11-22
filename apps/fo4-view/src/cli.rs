//! Command-line parsing, as pure functions.
use crate::fly::FlySettings;
use fo4_scene::RuleLevel;

pub const USAGE: &str = "\
usage:
  fo4-view MANIFEST DATA_DIRECTORY CELL_EDITOR_ID [--strict|--measured|--under-hypotheses]
           --eye X,Y,Z --look X,Y,Z [--backface-cull] [--terrain] [--around N]
           [--speed UNITS_PER_SECOND] [--fast-speed UNITS_PER_SECOND] [--radians-per-pixel R]
           [--walk] [--texture-cap N] [--alpha-tested-cap N] [--reference-textures]
           [--scene-memory MIB] [--no-stream]
           [--frame-log build/FRAMES.tsv] [--script-path X,Y;... [--script-shots build/NAME]]
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
      Measurement aids: --frame-log writes one line per frame (interval,
      time in the frame's update, cells drawn and uploading, entities).
      --script-path (with --walk) steers the walker through the waypoints
      as the headless --path does once the cells around the start are
      drawn, then returns to the start pose as R does and closes the
      window when its cells are drawn again; --script-shots captures the
      window at the start pose before the walk (NAME-start.png) and after
      the return (NAME-return.png), from the same camera pose.
  fo4-view MANIFEST DATA_DIRECTORY CELL_EDITOR_ID [level] --headless
           --eye X,Y,Z --look X,Y,Z --out build/FRAME.png [--tags build/TAGS.bin]
           [--size WxH] [--backface-cull] [--terrain] [--around N] [--walk]
           [--texture-cap N] [--alpha-tested-cap N] [--reference-textures]
           [--scene-memory MIB] [--path X,Y;X,Y;... [--start-pose]]
      One offscreen frame, no window. --tags also writes the per-pixel instance
      tags (u32 little-endian, u32::MAX = background). With --walk the walking
      world is built as in the window, the walker is dropped with its eye at
      --eye and settled, and the frame is drawn from its eye looking the
      same way; the settled pose is printed.
      --path (with --walk --terrain --around N) walks the settled walker to
      each X,Y in turn at the run speed, moving the loaded grid as the
      window does but loading synchronously, prints every grid change
      (cells dropped and loaded, time per cell, credits kept) and draws
      the frame from where the walk ends, looking along its last leg;
      with --start-pose, from the settled start pose instead (the frame
      without --path), with the cells kept when the walk ends.
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
    let mut debug = WindowDebug::default();
    let mut rest = args[3..].iter();
    while let Some(arg) = rest.next() {
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
            "--frame-log" => debug.frame_log = Some(value()?.clone()),
            "--script-path" => debug.path = Some(waypoints(value()?)?),
            "--script-shots" => debug.shots = Some(value()?.clone()),
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
    let scene = Scene {
        manifest: args[0].clone(),
        data: args[1].clone(),
        cell: args[2].clone(),
        level,
        eye: eye.ok_or_else(usage)?,
        look: look.ok_or_else(usage)?,
        backface_cull,
        terrain,
        around,
        textures,
        scene_memory: memory,
    };
    let grid_moves = walk && scene.terrain && scene.around > 0;
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
    } else {
        Ok(Command::Window {
            scene,
            fly,
            walk,
            stream: grid_moves && !no_stream,
            debug,
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
                },
                fly: DEFAULT_FLY,
                walk: false,
                stream: false,
                debug: WindowDebug::default(),
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
