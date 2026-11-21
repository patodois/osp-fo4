//! Command-line parsing, as pure functions.
use crate::fly::FlySettings;
use fo4_scene::RuleLevel;

pub const USAGE: &str = "\
usage:
  fo4-view MANIFEST DATA_DIRECTORY CELL_EDITOR_ID [--strict|--measured|--under-hypotheses]
           --eye X,Y,Z --look X,Y,Z [--backface-cull] [--terrain] [--around N]
           [--speed UNITS_PER_SECOND] [--fast-speed UNITS_PER_SECOND] [--radians-per-pixel R]
           [--walk]
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
      --terrain also draws the cell's own terrain, plain grey as the CPU
      render's --terrain (needs --under-hypotheses while its rules are pending).
      --around N (1 to 3) also loads the exterior cells within N grid steps:
      their placed objects are drawn and, with --walk, collided; with
      --terrain their terrain is drawn too.
  fo4-view MANIFEST DATA_DIRECTORY CELL_EDITOR_ID [level] --headless
           --eye X,Y,Z --look X,Y,Z --out build/FRAME.png [--tags build/TAGS.bin]
           [--size WxH] [--backface-cull] [--terrain] [--around N] [--walk]
      One offscreen frame, no window. --tags also writes the per-pixel instance
      tags (u32 little-endian, u32::MAX = background). With --walk the walking
      world is built as in the window, the walker is dropped with its eye at
      --eye and settled, and the frame is drawn from its eye looking the
      same way; the settled pose is printed.
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
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Window {
        scene: Scene,
        fly: FlySettings,
        /// Start in walk mode (`walk`), with Tab to switch.
        walk: bool,
    },
    Headless {
        scene: Scene,
        size: [u32; 2],
        out: String,
        tags: Option<String>,
        /// Settle a walker from the eye and draw from its eye.
        walk: bool,
    },
    Compare {
        cpu: String,
        gpu: String,
        diff: String,
        tags: Option<(String, String)>,
    },
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
            _ => return Err(usage()),
        }
    }
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
    };
    if headless {
        Ok(Command::Headless {
            scene,
            size: frame,
            out: out.ok_or_else(usage)?,
            tags,
            walk,
        })
    } else if out.is_some() || tags.is_some() || frame != fo4_scene::camera::REFERENCE_SIZE {
        Err("--out, --tags and --size apply to --headless only".into())
    } else {
        Ok(Command::Window { scene, fly, walk })
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
        } = c
        else {
            panic!("{c:?}")
        };
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
                },
                fly: DEFAULT_FLY,
                walk: false,
            }
        );
        let c = parse(&args(
            "m d C --strict --eye 1,2,3 --look 4,5,6 --speed 50 --fast-speed 75",
        ))
        .unwrap();
        let Command::Window { scene, fly, walk } = c else {
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
        assert!(matches!(&c, Command::Window { walk: true, scene, .. } if scene.around == 1));
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
