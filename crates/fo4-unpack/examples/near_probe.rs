#![forbid(unsafe_code)]
//! The placements of loaded cells near a point, for measurement notes:
//! `near_probe MANIFEST DATA_ROOT X Y Z RADIUS PLUGIN:HEXID...` loads each
//! cell as the viewer does (hypotheses level) and prints, nearest first,
//! every placement within RADIUS of the point (ground distance): its key,
//! base signature and editor id, model path, position, rotation, scale,
//! base bounds, whether the draw is ready, its blockers' codes, and its
//! enable parent. Output goes to standard output; it is never versioned.
use fo4_core::{Budget, Manifest};
use fo4_plugin::FormKey;
use fo4_scene::{Loader, RuleLevel};
use std::path::Path;

fn key(s: &str) -> Result<FormKey, String> {
    let (p, id) = s.split_once(':').ok_or("PLUGIN:HEXID")?;
    Ok(FormKey {
        plugin: p.to_ascii_lowercase(),
        object_id: u32::from_str_radix(id, 16).map_err(|e| e.to_string())?,
    })
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 7 {
        return Err("near_probe MANIFEST DATA_ROOT X Y Z RADIUS PLUGIN:HEXID...".into());
    }
    let mut manifest = Manifest::load(Path::new(&args[0]))?;
    manifest.root = Path::new(&args[1]).canonicalize()?;
    let p: Vec<f64> = args[2..5]
        .iter()
        .map(|v| v.parse())
        .collect::<Result<_, _>>()?;
    let radius: f64 = args[5].parse()?;
    let limits = manifest.limits.clone();
    let store =
        fo4_plugin::typed::Store::open(&manifest, &Budget::new(limits.max_override_memory))?;
    let vfs = fo4_vfs::Vfs::mount(&manifest, &Budget::new(limits.max_vfs_memory))?;
    let budget = Budget::metered(limits.max_memory);
    let mut loader = Loader::new(
        &store,
        &vfs,
        &manifest.language,
        RuleLevel::Hypotheses,
        &budget,
    );
    let mut rows = Vec::new();
    for c in &args[6..] {
        let scene = loader.cell(&key(c)?, &budget)?;
        for inst in &scene.instances {
            let Some(t) = inst.reference.data.transform else {
                continue;
            };
            let d = ((f64::from(t[0]) - p[0]).powi(2) + (f64::from(t[1]) - p[1]).powi(2)).sqrt();
            if d > radius {
                continue;
            }
            let (sig, edid, bounds) = match &inst.base {
                Some(b) => (
                    String::from_utf8_lossy(&b.record.signature).into_owned(),
                    b.common.editor_id.clone().unwrap_or_default(),
                    b.common.bounds,
                ),
                None => ("-".into(), String::new(), None),
            };
            let model = inst
                .assets
                .first()
                .map(|a| String::from_utf8_lossy(&loader.assets.nodes[*a].path).into_owned())
                .unwrap_or_default();
            let mut codes: Vec<&str> = inst.blockers.iter().map(|e| e.code.as_str()).collect();
            codes.sort_unstable();
            codes.dedup();
            rows.push((
                d,
                format!(
                    "{d:7.1}\t{}:{:08X}\t{sig}\t{edid}\t{model}\tpos {:.1} {:.1} {:.1}\trot {:.3} {:.3} {:.3}\tscale {:?}\tbounds {:?}\tready {}\tblockers {}\tparent {:?}\tcell {c}",
                    inst.reference.key.plugin,
                    inst.reference.key.object_id,
                    t[0],
                    t[1],
                    t[2],
                    t[3],
                    t[4],
                    t[5],
                    inst.reference.data.scale,
                    bounds,
                    inst.state.ready_at(RuleLevel::Hypotheses),
                    codes.join(","),
                    inst.reference.data.enable_parent.as_ref().map(|e| (&e.parent, e.flags)),
                ),
            ));
        }
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, r) in rows {
        println!("{r}");
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
