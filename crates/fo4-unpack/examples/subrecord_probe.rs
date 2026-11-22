#![forbid(unsafe_code)]
//! Values of chosen subrecords over every winning record of a type, for the
//! decisions on uninterpreted visual subrecords of `spec/scene.md`: sizes,
//! and, read as little-endian four-byte words, the distinct values of each
//! word position modulo a stride (resolved to a winning record's type and
//! editor ID when the word is a FormID of the record's plugin context, as a
//! float otherwise). Measurement support only: the output names records and
//! is kept outside the repository.
//!
//! `subrecord_probe MANIFEST OUT.txt STRIDE SIG.SUB [SIG.SUB...]`
use fo4_core::{Budget, Manifest};
use fo4_plugin::{typed::Store, FormKey};
use std::{collections::BTreeMap, fmt::Write as _, path::Path};

fn bump<K: Ord>(m: &mut BTreeMap<K, u64>, k: K) {
    *m.entry(k).or_default() += 1;
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        return Err("subrecord_probe MANIFEST OUT.txt STRIDE SIG.SUB [SIG.SUB...]".into());
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    let stride: usize = args[2].parse()?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let mut out = String::new();
    let describe = |plugin: usize, file: &str, word: u32| -> String {
        let b = Budget::new(m.limits.max_memory);
        let key: Option<FormKey> = store.order().remap(plugin, word, file, 0).ok().flatten();
        let Some(key) = key else {
            return format!("{word:08x} (f32 {})", f32::from_bits(word));
        };
        match store.raw(&key, &b) {
            Ok(Some(r)) => {
                let edid = r
                    .subrecords
                    .iter()
                    .find(|s| s.signature == *b"EDID")
                    .map(|s| {
                        let d = r.data(s);
                        String::from_utf8_lossy(d.strip_suffix(&[0]).unwrap_or(d)).into_owned()
                    })
                    .unwrap_or_default();
                format!(
                    "{} {} {:06x}",
                    String::from_utf8_lossy(&r.signature),
                    edid,
                    key.object_id
                )
            }
            _ => format!("{word:08x} (f32 {}; no record)", f32::from_bits(word)),
        }
    };
    for wanted in &args[3..] {
        let (s, sub) = wanted.split_once('.').ok_or("SIG.SUB")?;
        let s: [u8; 4] = s.as_bytes().try_into()?;
        let sub: [u8; 4] = sub.as_bytes().try_into()?;
        let keys: Vec<(FormKey, usize)> = store
            .winners()
            .filter(|w| w.signature == s)
            .map(|w| (store.overrides().form_key(w.key, store.order()), w.plugin))
            .collect();
        let mut sizes: BTreeMap<usize, u64> = BTreeMap::new();
        let mut words: BTreeMap<usize, BTreeMap<String, u64>> = BTreeMap::new();
        let mut records = 0u64;
        for (key, plugin) in &keys {
            let b = Budget::new(m.limits.max_memory);
            let Some(r) = store.raw(key, &b)? else {
                continue;
            };
            if r.deleted() {
                continue;
            }
            let mut has = false;
            for x in r.subrecords.iter().filter(|x| x.signature == sub) {
                has = true;
                let d = r.data(x);
                bump(&mut sizes, d.len());
                for (i, w) in d.as_chunks::<4>().0.iter().enumerate() {
                    let word = u32::from_le_bytes(*w);
                    let pos = (i * 4) % stride.max(4);
                    let text = if pos == 0 {
                        describe(*plugin, &r.source.file, word)
                    } else {
                        format!("{word:08x} (f32 {})", f32::from_bits(word))
                    };
                    bump(words.entry(pos).or_default(), text);
                }
            }
            records += u64::from(has);
        }
        writeln!(out, "## {wanted}: {records} records")?;
        writeln!(out, "sizes {sizes:?}")?;
        for (pos, values) in &words {
            writeln!(out, "word +{pos} (mod {stride}): {} distinct", values.len())?;
            let mut v: Vec<_> = values.iter().collect();
            v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            for (k, n) in v.iter().take(40) {
                writeln!(out, "  {n}\t{k}")?;
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
