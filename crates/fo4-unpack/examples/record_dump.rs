#![forbid(unsafe_code)]
//! Prints the subrecord sequence of winning records for measurement notes:
//! `record_dump MANIFEST cell EDITOR_ID RADIUS` lists the non-REFR children
//! of the cells around a cell; `record_dump MANIFEST form PLUGIN HEXID...`
//! dumps records. Output goes to standard output; it is never versioned.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{typed::Store, FormKey, Record};
use std::path::Path;

fn show(store: &Store, plugin: usize, r: &Record) {
    println!(
        "{} {:08x} flags {:08x}",
        String::from_utf8_lossy(&r.signature),
        r.form_id,
        r.flags
    );
    for s in &r.subrecords {
        let d = r.data(s);
        let printable = d.len() > 1
            && d.last() == Some(&0)
            && d[..d.len() - 1].iter().all(|c| (0x20..0x7f).contains(c));
        let text = if printable && d.len() != 4 {
            format!("\"{}\"", String::from_utf8_lossy(&d[..d.len() - 1]))
        } else {
            let mut t = String::new();
            for (i, c) in d.iter().take(64).enumerate() {
                if i % 4 == 0 && i > 0 {
                    t.push(' ');
                }
                t.push_str(&format!("{c:02x}"));
            }
            if d.len().is_multiple_of(4) && d.len() <= 64 {
                let mut refs = Vec::new();
                for o in (0..d.len()).step_by(4) {
                    let id = u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
                    if id == 0 {
                        continue;
                    }
                    if let Some(w) = store
                        .order()
                        .remap(plugin, id, &r.source.file, s.offset)
                        .ok()
                        .flatten()
                        .and_then(|k| store.overrides().winner(&k))
                    {
                        refs.push(format!("+{o}->{}", String::from_utf8_lossy(&w.signature)));
                    }
                }
                if !refs.is_empty() {
                    t.push_str(&format!("  [{}]", refs.join(" ")));
                }
            }
            t
        };
        println!(
            "  {} {:4} {}",
            String::from_utf8_lossy(&s.signature),
            d.len(),
            text
        );
    }
}

fn dump(store: &Store, key: &FormKey) -> Result<(), Box<dyn std::error::Error>> {
    let b = Budget::new(256 << 20);
    let Some(w) = store.overrides().winner(key) else {
        println!("missing {key:?}");
        return Ok(());
    };
    let plugin = w.plugin;
    if let Some(r) = store.raw(key, &b)? {
        println!(
            "== {}:{:06x} in {}",
            key.plugin, key.object_id, r.source.file
        );
        show(store, plugin, &r);
    }
    Ok(())
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let m = Manifest::load(Path::new(&args[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    match args[1].as_str() {
        "cell" => {
            let b = Budget::new(m.limits.max_memory);
            let cell = store.find_cell(&args[2], &b)?.ok_or("no cell")?;
            let radius: u32 = args[3].parse()?;
            let mut cells = vec![cell.clone()];
            if radius > 0 {
                let a = fo4_scene::grid::around(&store, &cell, radius, &b)?;
                cells.extend(a.neighbours.into_iter().filter_map(|n| n.cell));
            }
            for c in &cells {
                let ch = store.children(c, &b)?;
                for (kind, list) in [
                    ("active", &ch.active),
                    ("excluded", &ch.excluded),
                    ("disabled", &ch.disabled),
                    ("deleted", &ch.deleted),
                ] {
                    for x in list.iter().filter(|x| x.signature != *b"REFR") {
                        println!(
                            "cell {}:{:06x} {kind} {} {}:{:06x} group {}",
                            c.plugin,
                            c.object_id,
                            String::from_utf8_lossy(&x.signature),
                            x.key.plugin,
                            x.key.object_id,
                            x.group_kind
                        );
                    }
                }
            }
        }
        "form" => {
            for id in &args[3..] {
                let key = FormKey {
                    plugin: args[2].clone(),
                    object_id: u32::from_str_radix(id, 16)?,
                };
                dump(&store, &key)?;
            }
        }
        "sig" => {
            let wanted: [u8; 4] = args[2].as_bytes().try_into()?;
            let n: usize = args[3].parse()?;
            let keys: Vec<FormKey> = store
                .winners()
                .filter(|w| w.signature == wanted)
                .map(|w| store.overrides().form_key(w.key, store.order()))
                .take(n)
                .collect();
            for k in &keys {
                dump(&store, k)?;
            }
        }
        _ => return Err("mode".into()),
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
