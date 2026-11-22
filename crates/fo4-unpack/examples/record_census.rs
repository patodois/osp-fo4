#![forbid(unsafe_code)]
//! Census of the subrecords of chosen record types over every winning record:
//! counts, sizes, and for each four-byte slot of small fixed-size fields how
//! many values resolve to a winning record, and of which type. Text fields
//! report their extension. Measurement support only: it writes one TSV.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{typed::Store, FormKey};
use std::{collections::BTreeMap, fmt::Write as _, path::Path};

#[derive(Default)]
struct Field {
    records: u64,
    occurrences: u64,
    sizes: BTreeMap<usize, u64>,
    text: u64,
    extensions: BTreeMap<String, u64>,
    slots: BTreeMap<usize, Slot>,
    max_per_record: u64,
}
#[derive(Default)]
struct Slot {
    zero: u64,
    resolved: u64,
    unresolved: u64,
    targets: BTreeMap<String, u64>,
}

fn sig(s: &[u8; 4]) -> String {
    String::from_utf8_lossy(s).into_owned()
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        return Err("record_census MANIFEST OUT.tsv SIG [SIG...]".into());
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let mut out = String::new();
    for (s, n) in &store.record_types {
        writeln!(out, "## type {} {}", sig(s), n)?;
    }
    for wanted in &args[2..] {
        let wanted: [u8; 4] = wanted.as_bytes().try_into()?;
        let keys: Vec<(FormKey, usize)> = store
            .winners()
            .filter(|w| w.signature == wanted)
            .map(|w| (store.overrides().form_key(w.key, store.order()), w.plugin))
            .collect();
        let mut fields: BTreeMap<[u8; 4], Field> = BTreeMap::new();
        let mut flags: BTreeMap<u32, u64> = BTreeMap::new();
        let mut deleted = 0u64;
        for (key, plugin) in &keys {
            let b = Budget::new(m.limits.max_memory);
            let Some(r) = store.raw(key, &b)? else {
                continue;
            };
            if r.deleted() {
                deleted += 1;
                continue;
            }
            *flags.entry(r.flags).or_default() += 1;
            let mut per: BTreeMap<[u8; 4], u64> = BTreeMap::new();
            for s in &r.subrecords {
                *per.entry(s.signature).or_default() += 1;
                let f = fields.entry(s.signature).or_default();
                f.occurrences += 1;
                let data = r.data(s);
                *f.sizes.entry(data.len()).or_default() += 1;
                let printable = data.len() > 1
                    && data.len() != 4
                    && data.last() == Some(&0)
                    && data[..data.len() - 1]
                        .iter()
                        .all(|c| (0x20..0x7f).contains(c));
                if printable {
                    f.text += 1;
                    let t = String::from_utf8_lossy(&data[..data.len() - 1]).to_lowercase();
                    let ext = t
                        .rsplit_once('.')
                        .map(|(_, e)| e.to_owned())
                        .filter(|e| e.len() <= 4)
                        .unwrap_or_else(|| "-".into());
                    *f.extensions.entry(ext).or_default() += 1;
                    continue;
                }
                if data.len() % 4 == 0 && data.len() <= 256 {
                    for o in (0..data.len()).step_by(4) {
                        let id = u32::from_le_bytes(data[o..o + 4].try_into()?);
                        let slot = f.slots.entry(o).or_default();
                        if id == 0 {
                            slot.zero += 1;
                            continue;
                        }
                        let target = store
                            .order()
                            .remap(*plugin, id, &r.source.file, s.offset)
                            .ok()
                            .flatten()
                            .and_then(|k| store.overrides().winner(&k).map(|w| w.signature));
                        match target {
                            Some(t) => {
                                slot.resolved += 1;
                                *slot.targets.entry(sig(&t)).or_default() += 1;
                            }
                            None => slot.unresolved += 1,
                        }
                    }
                }
            }
            for (s, n) in per {
                let f = fields.get_mut(&s).expect("field");
                f.records += 1;
                f.max_per_record = f.max_per_record.max(n);
            }
        }
        let live = keys.len() as u64 - deleted;
        writeln!(
            out,
            "# {} winners {} deleted {} live {}",
            sig(&wanted),
            keys.len(),
            deleted,
            live
        )?;
        let top_flags: Vec<String> = {
            let mut v: Vec<_> = flags.iter().collect();
            v.sort_by_key(|a| std::cmp::Reverse(*a.1));
            v.iter()
                .take(8)
                .map(|(f, n)| format!("{f:08x}:{n}"))
                .collect()
        };
        writeln!(out, "# flags {}", top_flags.join(" "))?;
        for (s, f) in &fields {
            let mut sizes: Vec<_> = f.sizes.iter().collect();
            sizes.sort_by_key(|a| std::cmp::Reverse(*a.1));
            let sizes: Vec<String> = sizes
                .iter()
                .take(6)
                .map(|(k, v)| format!("{k}:{v}"))
                .collect();
            let ext: Vec<String> = f
                .extensions
                .iter()
                .map(|(k, v)| format!("{k}:{v}"))
                .collect();
            writeln!(
                out,
                "{}\t{}\trecords {}/{}\tocc {}\tmax/rec {}\tsizes {}{}",
                sig(&wanted),
                sig(s),
                f.records,
                live,
                f.occurrences,
                f.max_per_record,
                sizes.join(","),
                if f.text > 0 {
                    format!("\ttext {} ext {}", f.text, ext.join(","))
                } else {
                    String::new()
                }
            )?;
            for (o, slot) in f.slots.iter().take(12) {
                if slot.resolved == 0 {
                    continue;
                }
                let t: Vec<String> = slot
                    .targets
                    .iter()
                    .map(|(k, v)| format!("{k}:{v}"))
                    .collect();
                writeln!(
                    out,
                    "\t\t+{o}\tzero {}\tresolved {}\tunresolved {}\t{}",
                    slot.zero,
                    slot.resolved,
                    slot.unresolved,
                    t.join(",")
                )?;
            }
        }
    }
    std::fs::write(&args[1], out)?;
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
