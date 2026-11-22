#![forbid(unsafe_code)]
//! Census of the material swap records (MSWP) and of what names them, for
//! the material swap decision of `spec/scene.md` (Material swaps): the order
//! of the BNAM, SNAM, FNAM and CNAM subrecords, the FNAM texts (classes of
//! characters, whether they occur in the paths of their own record, in the
//! editor IDs of other records), duplicate sources, and the references and
//! base records that name each kind of swap. Writes an aggregate report and,
//! as a second file kept outside the repository, the FNAM texts with counts.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{typed::Store, FormKey};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::Path,
};

fn sig(s: &[u8; 4]) -> String {
    String::from_utf8_lossy(s).into_owned()
}
fn text(d: &[u8]) -> String {
    String::from_utf8_lossy(d.strip_suffix(&[0]).unwrap_or(d)).into_owned()
}
fn bump<K: Ord>(m: &mut BTreeMap<K, u64>, k: K) {
    *m.entry(k).or_default() += 1;
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        return Err("swap_census MANIFEST OUT.txt TEXTS.tsv".into());
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let winners: Vec<(FormKey, [u8; 4], usize)> = store
        .winners()
        .map(|w| {
            (
                store.overrides().form_key(w.key, store.order()),
                w.signature,
                w.plugin,
            )
        })
        .collect();
    let mut out = String::new();
    // Editor IDs of every winning record, lower case, for the FNAM check.
    let mut editor_ids: BTreeSet<String> = BTreeSet::new();
    // MSWP: key -> (has FNAM)
    let mut swaps: BTreeMap<FormKey, bool> = BTreeMap::new();
    let mut patterns: BTreeMap<String, u64> = BTreeMap::new();
    let mut fnam_position: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut fnam_texts: BTreeMap<String, u64> = BTreeMap::new();
    let mut fnam_classes: BTreeMap<String, u64> = BTreeMap::new();
    let mut fnam_in_paths = 0u64;
    let mut fnam_total = 0u64;
    let mut fnam_per_record: BTreeMap<usize, u64> = BTreeMap::new();
    let mut entries_per_record: BTreeMap<String, u64> = BTreeMap::new();
    let mut duplicate_sources = 0u64;
    let mut cnam_per_entry: BTreeMap<String, u64> = BTreeMap::new();
    let mut deleted = 0u64;
    let mut swap_count = 0u64;
    for (key, signature, _) in &winners {
        let b = Budget::new(m.limits.max_memory);
        let Some(r) = store.raw(key, &b)? else {
            continue;
        };
        for s in &r.subrecords {
            if s.signature == *b"EDID" {
                editor_ids.insert(text(r.data(s)).to_lowercase());
            }
        }
        if signature != b"MSWP" {
            continue;
        }
        if r.deleted() {
            deleted += 1;
            continue;
        }
        swap_count += 1;
        let mut pattern = String::new();
        let mut sources = BTreeSet::new();
        let mut paths = String::new();
        let mut fnams = Vec::new();
        let mut seen_bnam = false;
        let mut last = [0u8; 4];
        let mut entries = 0usize;
        let mut cnam_here = 0u64;
        for s in &r.subrecords {
            let d = r.data(s);
            match &s.signature {
                b"BNAM" => {
                    if entries > 0 {
                        bump(&mut cnam_per_entry, format!("{cnam_here}"));
                    }
                    cnam_here = 0;
                    seen_bnam = true;
                    entries += 1;
                    let t = text(d).to_lowercase();
                    if !sources.insert(t.clone()) {
                        duplicate_sources += 1;
                    }
                    paths.push_str(&t);
                    paths.push('\n');
                }
                b"SNAM" => {
                    paths.push_str(&text(d).to_lowercase());
                    paths.push('\n');
                }
                b"FNAM" => {
                    fnam_total += 1;
                    let pos = if !seen_bnam {
                        "before the first BNAM"
                    } else if last == *b"SNAM" {
                        "right after an SNAM"
                    } else if last == *b"CNAM" {
                        "right after a CNAM"
                    } else if last == *b"FNAM" {
                        "right after an FNAM"
                    } else {
                        "elsewhere"
                    };
                    bump(&mut fnam_position, pos);
                    let t = text(d);
                    let class = t
                        .chars()
                        .map(|c| {
                            if c.is_ascii_uppercase() {
                                'A'
                            } else if c.is_ascii_lowercase() {
                                'a'
                            } else if c.is_ascii_digit() {
                                '9'
                            } else {
                                c
                            }
                        })
                        .fold(String::new(), |mut a, c| {
                            if !a.ends_with(c) {
                                a.push(c);
                            }
                            a
                        });
                    bump(&mut fnam_classes, class);
                    fnams.push(t.clone());
                    bump(&mut fnam_texts, t);
                }
                b"CNAM" => cnam_here += 1,
                _ => {}
            }
            if matches!(&s.signature, b"BNAM" | b"SNAM" | b"FNAM" | b"CNAM") {
                if !pattern.ends_with(&sig(&s.signature)) || s.signature == *b"BNAM" {
                    pattern.push_str(&sig(&s.signature));
                    pattern.push(' ');
                }
                last = s.signature;
            }
        }
        if entries > 0 {
            bump(&mut cnam_per_entry, format!("{cnam_here}"));
        }
        for f in &fnams {
            if paths.contains(&f.to_lowercase()) {
                fnam_in_paths += 1;
            }
        }
        // Pattern with the entry repeated collapsed.
        let collapsed = pattern
            .replace("BNAM SNAM CNAM ", "(B S C) ")
            .replace("BNAM SNAM ", "(B S) ");
        let mut c = String::new();
        let mut prev = "";
        for w in collapsed.split(") ") {
            if w != prev {
                c.push_str(w);
                c.push_str(") ");
            } else if !c.ends_with("* ") {
                c.push_str("* ");
            }
            prev = w;
        }
        bump(&mut patterns, c.trim().to_string());
        bump(&mut fnam_per_record, fnams.len());
        bump(
            &mut entries_per_record,
            match entries {
                0 => "0".into(),
                1 => "1".into(),
                2..=4 => "2-4".into(),
                _ => "5+".into(),
            },
        );
        swaps.insert(key.clone(), !fnams.is_empty());
    }
    let fnam_editor_ids = fnam_texts
        .keys()
        .filter(|t| editor_ids.contains(&t.to_lowercase()))
        .count();
    writeln!(
        out,
        "MSWP winning {swap_count} (deleted {deleted}); with FNAM {}",
        swaps.values().filter(|v| **v).count()
    )?;
    writeln!(out, "entries (BNAM) per record:")?;
    for (k, n) in &entries_per_record {
        writeln!(out, "  {k}\t{n}")?;
    }
    writeln!(
        out,
        "subrecord order (B S = BNAM SNAM entry, C = CNAM; * repeated):"
    )?;
    let mut v: Vec<_> = patterns.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (k, n) in v.iter().take(20) {
        writeln!(out, "  {n}\t{k}")?;
    }
    writeln!(out, "  ({} patterns)", patterns.len())?;
    writeln!(out, "CNAM per entry:")?;
    for (k, n) in &cnam_per_entry {
        writeln!(out, "  {k}\t{n}")?;
    }
    writeln!(
        out,
        "entries whose BNAM repeats an earlier BNAM of the record\t{duplicate_sources}"
    )?;
    writeln!(out, "FNAM {fnam_total}; per record:")?;
    for (k, n) in &fnam_per_record {
        writeln!(out, "  {k}\t{n}")?;
    }
    writeln!(out, "FNAM position:")?;
    for (k, n) in &fnam_position {
        writeln!(out, "  {k}\t{n}")?;
    }
    writeln!(
        out,
        "FNAM distinct texts {}; equal to an editor ID of a winning record {fnam_editor_ids}; occurring in a BNAM or SNAM path of the same record {fnam_in_paths} of {fnam_total}",
        fnam_texts.len()
    )?;
    writeln!(
        out,
        "FNAM character classes (A upper, a lower, 9 digit; runs collapsed):"
    )?;
    let mut v: Vec<_> = fnam_classes.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (k, n) in v.iter().take(15) {
        writeln!(out, "  {n}\t{k}")?;
    }
    // Who names a swap: REFR XMSP, base MODS (any record type).
    let mut namers: BTreeMap<String, [u64; 2]> = BTreeMap::new();
    for (key, signature, plugin) in &winners {
        if !matches!(
            signature,
            b"REFR"
                | b"STAT"
                | b"SCOL"
                | b"MSTT"
                | b"ACTI"
                | b"DOOR"
                | b"FURN"
                | b"LIGH"
                | b"MISC"
                | b"CONT"
                | b"FLOR"
                | b"TERM"
                | b"ARMO"
                | b"WEAP"
                | b"ARMA"
        ) {
            continue;
        }
        let b = Budget::new(m.limits.max_memory);
        let Some(r) = store.raw(key, &b)? else {
            continue;
        };
        for s in &r.subrecords {
            if !matches!(&s.signature, b"XMSP" | b"MODS") {
                continue;
            }
            let d = r.data(s);
            if d.len() != 4 {
                namers
                    .entry(format!(
                        "{}.{} size {}",
                        sig(signature),
                        sig(&s.signature),
                        d.len()
                    ))
                    .or_default()[0] += 1;
                continue;
            }
            let id = u32::from_le_bytes(d.try_into()?);
            let target = store
                .order()
                .remap(*plugin, id, &r.source.file, 0)
                .ok()
                .flatten();
            let k = format!("{}.{}", sig(signature), sig(&s.signature));
            let e = namers.entry(k).or_default();
            match target.and_then(|t| swaps.get(&t)) {
                Some(true) => e[1] += 1,
                Some(false) => e[0] += 1,
                None => {}
            }
        }
    }
    writeln!(out, "records naming an MSWP (without FNAM / with FNAM):")?;
    for (k, n) in &namers {
        writeln!(out, "  {k}\t{}\t{}", n[0], n[1])?;
    }
    std::fs::write(&args[1], out)?;
    let mut t = String::from("fnam\tcount\n");
    for (k, n) in &fnam_texts {
        writeln!(t, "{k}\t{n}")?;
    }
    std::fs::write(&args[2], t)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
