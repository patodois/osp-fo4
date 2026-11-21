//! `walk`: drops the controller of `fo4-walk` into the placed collision of
//! one cell (`fo4_scene::collision`), lets it settle, walks a path of
//! relative legs and writes a deterministic trace (`trace.json`: position,
//! grounded flag and the instance stood on, per tick) beside the usual
//! summary. Every movement number is a provisional registered rule
//! (`spec/walk.md`); the command refuses to run unless the level accepts
//! them. As evidence only, it lists the game settings (GMST) whose EditorID
//! names movement, with their raw DATA bytes; none is applied.
use crate::Reports;
use fo4_core::{Budget, Error, Manifest, Result};
use fo4_plugin::typed::Store;
use fo4_plugin::FormKey;
use fo4_scene::collision::{self, BodyPlacement, CellCollision};
use fo4_scene::{Loader, RuleLevel};
use fo4_vfs::Vfs;
use fo4_walk::{rules, Character, Input, Params, RayHit, Settings, State, World};
use serde::Serialize;

/// Command-line arguments of `walk`.
#[derive(Debug, Clone, PartialEq)]
pub struct Args {
    /// Feet of the character at the start, world units.
    pub from: [f64; 3],
    /// Relative legs (dx, dy), walked in order.
    pub legs: Vec<[f64; 2]>,
    pub run: bool,
}

/// Most legs in one path.
pub const MAX_LEGS: usize = 1024;
/// Ticks allowed to settle before the path.
pub const SETTLE_TICKS: u64 = 600;
/// Most ticks in one trace.
pub const MAX_TICKS: u64 = 200_000;
/// A leg ends when it gets this close to its target (world units)...
pub const REACHED: f64 = 0.01;
/// ...or when the distance to the target shrinks by less than this over
/// [`STALL_TICKS`] ticks.
pub const STALL_DISTANCE: f64 = 0.5;
pub const STALL_TICKS: u64 = 30;
/// EditorID substrings (lower case) of the game settings listed as evidence.
pub const SETTING_KEYWORDS: [&str; 11] = [
    "jump", "walk", "run", "slope", "step", "gravity", "stair", "fall", "sprint", "movechar",
    "height",
];
/// Most game settings listed.
pub const MAX_SETTINGS: usize = 512;

#[derive(Debug, Clone, Serialize)]
pub struct ParamRow {
    pub name: &'static str,
    pub rule: &'static str,
    pub value: f64,
    pub origin: fo4_walk::Origin,
    pub status: String,
}

/// A surface under a point, with the instance it belongs to.
#[derive(Debug, Clone, Serialize)]
pub struct Surface {
    pub z: f64,
    pub normal: [f64; 3],
    #[serde(flatten)]
    pub owner: Owner,
}

/// The cell instance (or added set) a primitive comes from.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Owner {
    pub source: u32,
    pub instance: Option<usize>,
    pub reference: Option<FormKey>,
    pub base: Option<FormKey>,
    pub model: Option<String>,
    pub class: Option<&'static str>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub tick: u64,
    /// `None` while settling.
    pub leg: Option<usize>,
    pub feet: [f64; 3],
    pub grounded: bool,
    pub stepped: bool,
    pub blocked: bool,
    /// What the character stands on.
    pub ground: Option<Owner>,
    /// The last wall, ceiling or steep surface pushed against this tick.
    pub wall: Option<Owner>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Leg {
    pub delta: [f64; 2],
    pub target: [f64; 2],
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub ticks: u64,
    pub reached: bool,
    /// Horizontal distance left to the target.
    pub remaining: f64,
    pub stalled: bool,
    pub ended_grounded: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GameSetting {
    pub form: FormKey,
    pub editor_id: String,
    pub data_len: Option<usize>,
    pub data_hex: Option<String>,
    /// The four DATA bytes read as a little-endian f32: a raw
    /// reinterpretation, not an established layout; never applied.
    pub raw_f32_le: Option<f32>,
}

#[derive(Default, Serialize)]
pub struct Summary {
    pub cell: Option<FormKey>,
    pub editor_id: Option<String>,
    pub level: Option<RuleLevel>,
    pub body_placement: Option<BodyPlacement>,
    pub collision_instances: u64,
    pub unplaced: u64,
    pub world: Option<fo4_walk::world::Stats>,
    /// Every registered rule the walk relies on, sorted.
    pub rules: Vec<&'static str>,
    pub params: Vec<ParamRow>,
    pub settings: Option<Settings>,
    pub from: [f64; 3],
    pub surface_under_start: Option<Surface>,
    pub settle_ticks: u64,
    pub settled: Option<[f64; 3]>,
    pub settled_grounded: bool,
    /// Settled feet z minus the surface found under the start point.
    pub settled_height_above_surface: Option<f64>,
    pub legs: Vec<Leg>,
    pub ticks: u64,
    pub grounded_ticks: u64,
    pub stepped_ticks: u64,
    pub blocked_ticks: u64,
    pub game_settings_scanned: u64,
    pub game_settings_matching: u64,
    pub fingerprint_seconds: f64,
    pub scene_seconds: f64,
    pub collision_seconds: f64,
    pub walk_seconds: f64,
}

/// The deterministic part of the report, written as `trace.json`.
#[derive(Serialize)]
struct Trace<'a> {
    schema: u32,
    editor_id: &'a str,
    level: RuleLevel,
    body_placement: Option<BodyPlacement>,
    rules: &'a [&'static str],
    params: &'a [ParamRow],
    settings: Settings,
    world: &'a fo4_walk::world::Stats,
    from: [f64; 3],
    path: &'a [[f64; 2]],
    run: bool,
    surface_under_start: &'a Option<Surface>,
    settle_ticks: u64,
    legs: &'a [Leg],
    steps: &'a [Step],
}

fn status_name(rule: &str) -> String {
    match fo4_scene::hypotheses::status(rule) {
        Some(s) => format!("{s:?}").to_lowercase(),
        None => "unregistered".into(),
    }
}

fn owner(c: &CellCollision, w: &World, source: u32) -> Owner {
    let s = w.sources.get(source as usize);
    let instance = s.and_then(|s| s.instance);
    let inst = instance.and_then(|i| c.instances.get(i));
    Owner {
        source,
        instance,
        reference: inst.map(|i| c.placement(i).reference.clone()),
        base: inst.map(|i| c.placement(i).base.clone()),
        model: inst.map(|i| c.model(i).path.clone()),
        class: s.and_then(|s| s.class).map(|k| k.class()),
        label: s.and_then(|s| s.label.clone()),
    }
}

fn surface(c: &CellCollision, w: &World, h: RayHit) -> Surface {
    Surface {
        z: h.z,
        normal: h.normal,
        owner: owner(c, w, h.source),
    }
}

pub fn run(
    m: &Manifest,
    editor_id: &str,
    r: &mut Reports,
    level: RuleLevel,
    overlay: Option<&std::path::Path>,
    args: &Args,
) -> Result<()> {
    r.items_enabled = true;
    r.summary.rule_level = Some(level);
    let params = Params::PROVISIONAL;
    let settings = Settings::DEFAULT;
    let mut own: Vec<&'static str> = rules::PARAMS.to_vec();
    own.push(rules::TRIANGLE_SIDES);
    fo4_walk::accept(&own, level, fo4_scene::hypotheses::status)?;
    let started = std::time::Instant::now();
    let store = Store::open_overlay(m, overlay, &Budget::new(m.limits.max_override_memory))?;
    for error in &store.diagnostics {
        r.error(error)?;
    }
    let vfs = Vfs::mount(m, &Budget::new(m.limits.max_vfs_memory))?;
    vfs.check_complete()?;
    r.summary.fingerprint = Some(vfs.fingerprint()?.to_owned());
    r.summary.fingerprint_scope = Some("assets+plugins".into());
    let fingerprint_seconds = started.elapsed().as_secs_f64();
    let started = std::time::Instant::now();
    let budget = Budget::new(m.limits.max_memory);
    let mut loader = Loader::new(&store, &vfs, &m.language, level, &budget);
    let scene = loader.by_editor_id(editor_id, &budget)?;
    let scene_seconds = started.elapsed().as_secs_f64();
    let started = std::time::Instant::now();
    let options = collision::Options {
        level,
        body_placement: None,
    };
    let c = collision::cell(&scene, &loader, &m.limits, options, &budget)?;
    let placement = c.body_placement.ok_or_else(|| {
        Error::new(
            "BodyPlacementRuleNotAccepted",
            "walk",
            0,
            "a selectable body placement level",
            format!("{level:?}"),
        )
    })?;
    let world = World::build(&c, placement, &budget)?;
    let collision_seconds = started.elapsed().as_secs_f64();
    let mut all = world.rules.clone();
    all.extend_from_slice(&rules::PARAMS);
    all.sort_unstable();
    all.dedup();
    fo4_walk::accept(&all, level, fo4_scene::hypotheses::status)?;
    let started = std::time::Instant::now();
    let mut s = Summary {
        cell: Some(scene.cell.key.clone()),
        editor_id: Some(editor_id.to_owned()),
        level: Some(level),
        body_placement: Some(placement),
        collision_instances: c.instances.len() as u64,
        unplaced: c.unplaced.len() as u64,
        world: Some(world.stats.clone()),
        rules: all,
        params: params
            .provenance()
            .into_iter()
            .map(|(name, rule, value, origin)| ParamRow {
                name,
                rule,
                value,
                origin,
                status: status_name(rule),
            })
            .collect(),
        settings: Some(settings),
        from: args.from,
        fingerprint_seconds,
        scene_seconds,
        collision_seconds,
        ..Summary::default()
    };
    let (steps, legs) = walk(&c, &world, params, settings, args, &mut s)?;
    s.walk_seconds = started.elapsed().as_secs_f64();
    let trace = Trace {
        schema: 1,
        editor_id,
        level,
        body_placement: Some(placement),
        rules: &s.rules,
        params: &s.params,
        settings,
        world: &world.stats,
        from: args.from,
        path: &args.legs,
        run: args.run,
        surface_under_start: &s.surface_under_start,
        settle_ticks: s.settle_ticks,
        legs: &legs,
        steps: &steps,
    };
    let mut text = serde_json::to_string(&trace)
        .map_err(|e| Error::new("ReportWrite", "trace.json", 0, "JSON", e))?;
    text.push('\n');
    r.tsv = Some(("trace.json", text));
    for leg in &legs {
        r.item(&serde_json::json!({"kind":"walk_leg","leg":leg}))?;
    }
    let (scanned, found) = game_settings(&store, &budget)?;
    s.game_settings_scanned = scanned;
    s.game_settings_matching = found.len() as u64;
    for g in found.iter().take(MAX_SETTINGS) {
        r.item(&serde_json::json!({"kind":"game_setting","setting":g}))?;
    }
    r.item(&serde_json::json!({"kind":"walk_context","cell":scene.cell.key,"editor_id":editor_id,"level":level,"body_placement":placement,"setting_keywords":SETTING_KEYWORDS}))?;
    r.summary.files_attempted = s.ticks;
    r.summary.files_valid = s.ticks;
    s.legs = legs;
    r.summary.walk = Some(s);
    Ok(())
}

/// Settles the character at `args.from` and walks every leg.
pub fn walk(
    c: &CellCollision,
    world: &World,
    params: Params,
    settings: Settings,
    args: &Args,
    s: &mut Summary,
) -> Result<(Vec<Step>, Vec<Leg>)> {
    let mut ch = Character::new(params, settings, args.from)?;
    let budget_ticks = |n: u64| fo4_core::limit(n, MAX_TICKS, "walk", 0);
    s.surface_under_start = world
        .ray_down(args.from[0], args.from[1], args.from[2] + 1e-6)
        .map(|h| surface(c, world, h));
    let mut steps = Vec::new();
    let record = |st: &State, leg: Option<usize>, steps: &mut Vec<Step>| {
        steps.push(Step {
            tick: st.tick,
            leg,
            feet: st.feet,
            grounded: st.grounded,
            stepped: st.stepped,
            blocked: st.blocked,
            ground: st.ground.map(|g| owner(c, world, g.source)),
            wall: st.wall.map(|g| owner(c, world, g.source)),
        });
    };
    let mut still = 0;
    for _ in 0..SETTLE_TICKS {
        let before = ch.feet;
        let st = ch.tick(world, Input::default());
        record(&st, None, &mut steps);
        s.settle_ticks += 1;
        let moved = fo4_walk::geometry::length(fo4_walk::geometry::sub(st.feet, before));
        if st.grounded && moved < 1e-9 {
            still += 1;
            if still == 2 {
                break;
            }
        } else {
            still = 0;
        }
    }
    s.settled = Some(ch.feet);
    s.settled_grounded = ch.grounded;
    s.settled_height_above_surface = s.surface_under_start.as_ref().map(|h| ch.feet[2] - h.z);
    let speed = if args.run {
        params.run_speed
    } else {
        params.walk_speed
    };
    let per_tick = speed * settings.tick;
    let mut legs = Vec::new();
    for (li, d) in args.legs.iter().enumerate() {
        let start = ch.feet;
        let target = [start[0] + d[0], start[1] + d[1]];
        let len = d[0].hypot(d[1]);
        let most = (len / per_tick).ceil() as u64 * 4 + 120;
        let remaining = |f: [f64; 3]| (target[0] - f[0]).hypot(target[1] - f[1]);
        let mut ticks = 0;
        let mut stalled = false;
        let mut mark = remaining(ch.feet);
        while ticks < most {
            let left = remaining(ch.feet);
            if left <= REACHED {
                break;
            }
            budget_ticks(steps.len() as u64 + 1)?;
            let heading = (target[1] - ch.feet[1]).atan2(target[0] - ch.feet[0]);
            let input = Input {
                forward: (left / per_tick).min(1.0),
                right: 0.0,
                heading,
                run: args.run,
            };
            let st = ch.tick(world, input);
            record(&st, Some(li), &mut steps);
            ticks += 1;
            if ticks % STALL_TICKS == 0 {
                let now = remaining(ch.feet);
                if mark - now < STALL_DISTANCE {
                    stalled = true;
                    break;
                }
                mark = now;
            }
        }
        let left = remaining(ch.feet);
        legs.push(Leg {
            delta: *d,
            target,
            start,
            end: ch.feet,
            ticks,
            reached: left <= REACHED,
            remaining: left,
            stalled,
            ended_grounded: ch.grounded,
        });
    }
    s.ticks = steps.len() as u64;
    for st in &steps {
        s.grounded_ticks += u64::from(st.grounded);
        s.stepped_ticks += u64::from(st.stepped);
        s.blocked_ticks += u64::from(st.blocked);
    }
    Ok((steps, legs))
}

/// Winning GMST records whose EditorID contains one of
/// [`SETTING_KEYWORDS`], with their DATA bytes as stored.
pub fn game_settings(store: &Store, b: &Budget) -> Result<(u64, Vec<GameSetting>)> {
    let mut scanned = 0;
    let mut out = Vec::new();
    let keys: Vec<FormKey> = store
        .winners()
        .filter(|o| o.signature == *b"GMST" && !o.deleted)
        .map(|o| store.overrides().form_key(o.key, store.order()))
        .collect();
    for key in keys {
        scanned += 1;
        let work = b.child();
        let Some(record) = store.raw(&key, &work)? else {
            continue;
        };
        let field = |sig: &[u8; 4]| {
            record
                .subrecords
                .iter()
                .find(|s| &s.signature == sig)
                .map(|s| record.data(s))
        };
        let Some(edid) = field(b"EDID") else { continue };
        let name: String = edid
            .iter()
            .take_while(|c| **c != 0)
            .map(|c| char::from(*c))
            .collect();
        let lower = name.to_ascii_lowercase();
        if !SETTING_KEYWORDS.iter().any(|k| lower.contains(k)) {
            continue;
        }
        let data = field(b"DATA");
        let raw_f32_le = data
            .filter(|d| d.len() == 4)
            .map(|d| f32::from_le_bytes([d[0], d[1], d[2], d[3]]));
        b.reserve(&mut out, 1, "walk game settings", 0)?;
        out.push(GameSetting {
            form: key,
            editor_id: name,
            data_len: data.map(<[u8]>::len),
            data_hex: data.map(|d| d.iter().map(|x| format!("{x:02x}")).collect()),
            raw_f32_le,
        });
    }
    out.sort_by(|a, b| a.editor_id.cmp(&b.editor_id));
    Ok((scanned, out))
}

/// `--from x,y,z`.
pub fn parse_point(text: &str) -> Option<[f64; 3]> {
    let v: Vec<f64> = text
        .split(',')
        .map(|x| x.trim().parse::<f64>().ok().filter(|x| x.is_finite()))
        .collect::<Option<_>>()?;
    (v.len() == 3).then(|| [v[0], v[1], v[2]])
}

/// `--path dx,dy[,dx,dy...]`.
pub fn parse_path(text: &str) -> Option<Vec<[f64; 2]>> {
    let v: Vec<f64> = text
        .split(',')
        .map(|x| x.trim().parse::<f64>().ok().filter(|x| x.is_finite()))
        .collect::<Option<_>>()?;
    let (pairs, rest) = v.as_chunks::<2>();
    if pairs.is_empty() || !rest.is_empty() || pairs.len() > MAX_LEGS {
        return None;
    }
    Some(pairs.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn points_and_paths_parse_strictly() {
        assert_eq!(parse_point("1,-2.5,3"), Some([1.0, -2.5, 3.0]));
        for bad in ["1,2", "1,2,3,4", "1,x,3", "1,2,inf", ""] {
            assert_eq!(parse_point(bad), None, "{bad}");
        }
        assert_eq!(
            parse_path("10,0,0,-5"),
            Some(vec![[10.0, 0.0], [0.0, -5.0]])
        );
        for bad in ["10", "10,0,1", "a,b", "", "1,NaN"] {
            assert_eq!(parse_path(bad), None, "{bad}");
        }
        let long = vec!["1"; 2 * MAX_LEGS + 2].join(",");
        assert_eq!(parse_path(&long), None);
    }

    #[test]
    fn walking_an_empty_world_falls_and_a_floor_holds() {
        let c = CellCollision {
            level: RuleLevel::Hypotheses,
            body_placement: Some(BodyPlacement::Ignored),
            models: Vec::new(),
            placements: Vec::new(),
            instances: Vec::new(),
            unplaced: Vec::new(),
            without_collision: Vec::new(),
        };
        let b = Budget::new(64 << 20);
        let mut w = World::new();
        let args = Args {
            from: [0.0, 0.0, 10.0],
            legs: vec![[50.0, 0.0], [0.0, 30.0]],
            run: false,
        };
        let mut s = Summary::default();
        let (steps, legs) = walk(
            &c,
            &w,
            Params::PROVISIONAL,
            Settings::DEFAULT,
            &args,
            &mut s,
        )
        .unwrap();
        assert_eq!(s.settle_ticks, SETTLE_TICKS);
        assert!(!s.settled_grounded);
        assert!(s.surface_under_start.is_none());
        assert!(legs.iter().all(|l| !l.reached));
        assert_eq!(steps.len() as u64, s.ticks);
        let quad = |z: f64| {
            let (a, b2, c2, d) = (
                [-500.0, -500.0, z],
                [500.0, -500.0, z],
                [500.0, 500.0, z],
                [-500.0, 500.0, z],
            );
            [[a, b2, c2], [a, c2, d]]
        };
        let src = w.add_triangles(&quad(0.0), "floor", &b).unwrap();
        let mut s = Summary::default();
        let (steps, legs) = walk(
            &c,
            &w,
            Params::PROVISIONAL,
            Settings::DEFAULT,
            &args,
            &mut s,
        )
        .unwrap();
        assert!(s.settled_grounded);
        assert!(s
            .settled_height_above_surface
            .is_some_and(|h| h.abs() < 1e-9));
        assert!(legs.iter().all(|l| l.reached && l.ended_grounded));
        let end = legs[1].end;
        assert!((end[0] - 50.0).abs() < 0.02 && (end[1] - 30.0).abs() < 0.02);
        let last = steps.last().unwrap();
        assert_eq!(last.ground.as_ref().map(|g| g.source), Some(src));
        assert_eq!(
            last.ground.as_ref().and_then(|g| g.label.as_deref()),
            Some("floor")
        );
    }
}
