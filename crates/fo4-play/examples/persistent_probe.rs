#![forbid(unsafe_code)]
//! The worldspace's persistent references near a point, for measurement
//! notes: `persistent_probe MANIFEST CELL_KEY X Y RADIUS` takes the
//! worldspace of the exterior cell `CELL_KEY` and prints, nearest first,
//! every reference of its cells' persistent groups whose stored position
//! lies within RADIUS (ground distance) of X, Y: its key, its record flags,
//! its base's signature, editor id and model path, and its position. These
//! are the references `near_probe` does not see (a cell load reads its own
//! groups only). Output goes to standard output; it is never versioned.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{typed::Store, FormKey};
use std::path::Path;

type E = Box<dyn std::error::Error>;

fn key(s: &str) -> Result<FormKey, String> {
    let (p, id) = s.split_once(':').ok_or("PLUGIN:HEXID")?;
    Ok(FormKey {
        plugin: p.to_ascii_lowercase(),
        object_id: u32::from_str_radix(id, 16).map_err(|e| e.to_string())?,
    })
}

fn run() -> Result<(), E> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [manifest, cell, x, y, radius] = a.as_slice() else {
        return Err("persistent_probe MANIFEST CELL_KEY X Y RADIUS".into());
    };
    let (x, y, radius): (f64, f64, f64) = (x.parse()?, y.parse()?, radius.parse()?);
    let m = Manifest::load(Path::new(manifest))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let b = Budget::new(m.limits.max_memory);
    let space = store
        .world(&key(cell)?)
        .ok_or("the cell is not an exterior cell of a worldspace")?;
    let g = fo4_scene::grid::grid_of(&store, &key(cell)?, &b)?.ok_or("the cell has no grid")?;
    let mut rows = Vec::new();
    for k in fo4_play::world::persistent_refs(&store, &space, g, 1, &b)? {
        let rb = b.child();
        let Some((r, _)) = store.raw_with_plugin(&k, &rb)? else {
            continue;
        };
        let Some(refr) = store.get::<fo4_plugin::typed::Refr>(&k, &rb)? else {
            continue;
        };
        let Some(t) = refr.data.transform else {
            continue;
        };
        let at = [t[0], t[1], t[2]].map(f64::from);
        let d = (at[0] - x).hypot(at[1] - y);
        if d > radius {
            continue;
        }
        let base = fo4_vm::bind::base_of(&store, &k, &rb).ok().flatten();
        let (sig, edid, model) = match base.as_ref() {
            Some(bk) => match store.raw_with_plugin(bk, &rb)? {
                Some((br, _)) => {
                    let text = |tag: &[u8; 4]| {
                        br.subrecords
                            .iter()
                            .find(|s| &s.signature == tag)
                            .map(|s| {
                                let d = br.data(s);
                                String::from_utf8_lossy(d.strip_suffix(&[0]).unwrap_or(d))
                                    .into_owned()
                            })
                            .unwrap_or_default()
                    };
                    (
                        String::from_utf8_lossy(&br.signature).into_owned(),
                        text(b"EDID"),
                        text(b"MODL"),
                    )
                }
                None => ("-".into(), String::new(), String::new()),
            },
            None => ("-".into(), String::new(), String::new()),
        };
        rows.push((
            d,
            format!(
                "{d:.1}\t{}\tflags {:08x}\t{sig}\t{edid}\t{model}\tpos {:.1} {:.1} {:.1}",
                fo4_vm::bind::record_key(&k),
                r.flags,
                at[0],
                at[1],
                at[2]
            ),
        ));
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, l) in rows {
        println!("{l}");
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("persistent_probe: {e}");
        std::process::exit(1);
    }
}
