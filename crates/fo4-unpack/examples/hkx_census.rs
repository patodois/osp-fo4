#![forbid(unsafe_code)]
//! Census of the winning `.hkx` files of the manifest's mounts with the
//! collision payload's container reader (`fo4_havok::Container`): which
//! read, the root object's class and the classes their objects carry, by
//! folder; and the `.hkx` paths that RACE records name, resolved the same
//! way. Measurement support for the animation survey; writes plain text.
use fo4_core::{Budget, Manifest};
use fo4_vfs::Vfs;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::Path,
};

fn bump<K: Ord>(m: &mut BTreeMap<K, u64>, k: K) {
    *m.entry(k).or_default() += 1;
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("hkx_census MANIFEST OUT.txt".into());
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let mut paths: Vec<Vec<u8>> = vfs
        .candidates
        .iter()
        .filter_map(|c| c.key.clone())
        .filter(|k| k.ends_with(b".hkx"))
        .collect();
    paths.sort();
    paths.dedup();
    let mut outcome: BTreeMap<String, u64> = BTreeMap::new();
    let mut versions: BTreeMap<String, u64> = BTreeMap::new();
    let mut roots: BTreeMap<String, u64> = BTreeMap::new();
    let mut roots_by_folder: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut files_with_class: BTreeMap<String, u64> = BTreeMap::new();
    let mut objects_of_class: BTreeMap<String, u64> = BTreeMap::new();
    let mut prefixes: BTreeMap<String, u64> = BTreeMap::new();
    let mut sizes = (0u64, 0u64);
    let mut word60: BTreeMap<(bool, String), u64> = BTreeMap::new();
    let mut errors_by_folder: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut classnames_at: BTreeMap<i64, u64> = BTreeMap::new();
    let mut shifted_names: BTreeMap<String, u64> = BTreeMap::new();
    for p in &paths {
        let name = String::from_utf8_lossy(p).into_owned();
        // Folder: the first three path components, which separate animation,
        // behaviour and character files of each actor.
        let folder = name
            .split('/')
            .take(name.split('/').count().saturating_sub(1).min(4))
            .collect::<Vec<_>>()
            .join("/");
        let folder = if name.contains("/animations/") {
            format!(
                "{}/.../animations",
                name.split('/').take(3).collect::<Vec<_>>().join("/")
            )
        } else {
            folder
        };
        let b = Budget::new(m.limits.max_memory);
        let bytes = match vfs.read_path_bytes(p, &b) {
            Ok(v) => v,
            Err(e) => {
                bump(&mut outcome, format!("read error {}", e.code));
                continue;
            }
        };
        sizes.0 += bytes.len() as u64;
        sizes.1 = sizes.1.max(bytes.len() as u64);
        bump(
            &mut prefixes,
            bytes
                .iter()
                .take(8)
                .map(|c| format!("{c:02x}"))
                .collect::<String>(),
        );
        let word15 = bytes
            .get(60..64)
            .map_or(0, |v| u32::from_le_bytes([v[0], v[1], v[2], v[3]]));
        let readable = fo4_havok::Container::read(&bytes, &name, 0, &b).is_ok();
        bump(&mut word60, (readable, format!("{word15:08x}")));
        match fo4_havok::Container::read(&bytes, &name, 0, &b) {
            Ok(c) => {
                bump(&mut outcome, "read".into());
                bump(&mut versions, c.version.to_string());
                let root = c
                    .root()
                    .map_or("<none>".to_string(), |o| o.class.to_string());
                bump(&mut roots, root.clone());
                bump(&mut roots_by_folder, (folder, root));
                let mut seen = BTreeSet::new();
                for o in &c.objects {
                    bump(&mut objects_of_class, o.class.to_string());
                    seen.insert(o.class.to_string());
                }
                for s in seen {
                    bump(&mut files_with_class, s);
                }
            }
            Err(e) => {
                bump(&mut outcome, format!("container error {}", e.code));
                bump(
                    &mut errors_by_folder,
                    (
                        folder,
                        format!(
                            "{} at {}: expected {}, found {}",
                            e.code, e.source.offset, e.expected, e.found
                        ),
                    ),
                );
                // Header words of a refused file, for the survey.
                if errors_by_folder.len() < 2 {
                    let w: Vec<String> = bytes
                        .chunks(4)
                        .take(16)
                        .map(|c| format!("{:08x}", u32::from_le_bytes([c[0], c[1], c[2], c[3]])))
                        .collect();
                    let next: String = bytes
                        .iter()
                        .skip(64)
                        .take(96)
                        .map(|c| {
                            if (0x20..0x7f).contains(c) {
                                *c as char
                            } else {
                                '.'
                            }
                        })
                        .collect();
                    eprintln!(
                        "{name}: header words {}; bytes 64..160 as text: {next}",
                        w.join(" ")
                    );
                }
                // Where the first section name sits in refused files.
                let at = bytes.windows(14).position(|w| w == b"__classnames__");
                bump(&mut classnames_at, at.map_or(-1, |v| v as i64));
                // Class names of a refused file whose first record sits at 80,
                // read as the container reader reads them at 64: start word at
                // record + 20, length at record + 44, entries of u32 signature,
                // byte 9 and a NUL-terminated name, then 0xff fill.
                if at == Some(80) {
                    let w = |o: usize| {
                        bytes
                            .get(o..o + 4)
                            .map(|v| u32::from_le_bytes([v[0], v[1], v[2], v[3]]) as usize)
                    };
                    if let (Some(start), Some(len)) = (w(100), w(124)) {
                        let end = (start + len).min(bytes.len());
                        let mut p = start;
                        let mut names = BTreeSet::new();
                        while p + 6 < end && bytes[p] != 0xff {
                            if bytes[p + 4] != 9 {
                                break;
                            }
                            let rest = &bytes[p + 5..end];
                            let Some(nul) = rest.iter().position(|c| *c == 0) else {
                                break;
                            };
                            names.insert(String::from_utf8_lossy(&rest[..nul]).into_owned());
                            p += 5 + nul + 1;
                        }
                        let key = names.into_iter().collect::<Vec<_>>().join(", ");
                        bump(&mut shifted_names, key);
                    }
                }
            }
        }
    }
    let mut out = String::new();
    writeln!(
        out,
        "winning .hkx paths: {}; bytes in all {}, largest {}",
        paths.len(),
        sizes.0,
        sizes.1
    )?;
    writeln!(out, "outcome: {outcome:?}")?;
    writeln!(out, "first eight bytes: {prefixes:?}")?;
    writeln!(out, "version text: {versions:?}")?;
    writeln!(
        out,
        "header word at byte 60 by whether the container reader accepts the file: {word60:?}"
    )?;
    writeln!(out, "\nroot object class (files):")?;
    let mut v: Vec<_> = roots.iter().collect();
    v.sort_by_key(|a| std::cmp::Reverse(*a.1));
    for (k, n) in v {
        writeln!(out, "  {n}\t{k}")?;
    }
    writeln!(
        out,
        "\nfiles holding an object of each class, and objects of that class:"
    )?;
    let mut v: Vec<_> = files_with_class.iter().collect();
    v.sort_by_key(|a| std::cmp::Reverse(*a.1));
    for (k, n) in v {
        writeln!(out, "  {n}\t{}\t{k}", objects_of_class[k])?;
    }
    writeln!(
        out,
        "\nroot class by folder (folders with at least 20 files):"
    )?;
    let mut per_folder: BTreeMap<&String, u64> = BTreeMap::new();
    for ((f, _), n) in &roots_by_folder {
        *per_folder.entry(f).or_default() += n;
    }
    for ((f, r), n) in &roots_by_folder {
        if per_folder[f] >= 20 {
            writeln!(out, "  {f}\t{r}\t{n}")?;
        }
    }
    writeln!(
        out,
        "\nrefused files: offset of the text __classnames__: {classnames_at:?}"
    )?;
    writeln!(
        out,
        "refused files: class-name lists of their __classnames__ section (files):"
    )?;
    let mut v: Vec<_> = shifted_names.iter().collect();
    v.sort_by_key(|a| std::cmp::Reverse(*a.1));
    for (k, n) in v {
        writeln!(out, "  {n}\t{k}")?;
    }
    writeln!(out, "\ncontainer errors by folder:")?;
    for ((f, e), n) in &errors_by_folder {
        writeln!(out, "  {n}\t{f}\t{e}")?;
    }
    // The .hkx paths of RACE records (the NAM3 group's MODL after MNAM and
    // FNAM; SGNM, repeated) against the files above.
    let store = fo4_plugin::typed::Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let mut race_paths: BTreeMap<String, (u64, BTreeSet<String>)> = BTreeMap::new();
    for w in store
        .winners()
        .filter(|w| w.signature == *b"RACE" && !w.deleted)
    {
        let key = store.overrides().form_key(w.key, store.order());
        let b = Budget::new(m.limits.max_memory);
        let Some(r) = store.raw(&key, &b)? else {
            continue;
        };
        let mut group = "";
        let mut marker = "";
        for sub in &r.subrecords {
            match &sub.signature {
                b"NAM1" => group = "NAM1",
                b"NAM3" => group = "NAM3",
                b"MNAM" => marker = "MNAM",
                b"FNAM" => marker = "FNAM",
                _ => {}
            }
            let field = match (&sub.signature, group) {
                (b"MODL", "NAM3") => format!("NAM3 {marker} MODL"),
                (b"SGNM", _) => "SGNM".to_string(),
                _ => continue,
            };
            let d = r.data(sub);
            let text = String::from_utf8_lossy(d.strip_suffix(&[0]).unwrap_or(d))
                .to_lowercase()
                .replace('\\', "/");
            let path = format!("meshes/{text}");
            let state = if !paths.iter().any(|p| p.as_slice() == path.as_bytes()) {
                "not in the archives"
            } else {
                let b = Budget::new(m.limits.max_memory);
                match vfs.read_path_bytes(path.as_bytes(), &b).map(|bytes| {
                    fo4_havok::Container::read(&bytes, &path, 0, &b).map(|c| {
                        c.objects
                            .iter()
                            .map(|o| o.class.to_string())
                            .collect::<BTreeSet<_>>()
                    })
                }) {
                    Ok(Ok(classes)) if classes.contains("hkbProjectData") => "read: hkbProjectData",
                    Ok(Ok(classes)) if classes.contains("hkbBehaviorGraph") => {
                        "read: hkbBehaviorGraph"
                    }
                    Ok(Ok(_)) => "read: other root",
                    Ok(Err(_)) => "refused by the container reader",
                    Err(_) => "unreadable",
                }
            };
            let e = race_paths.entry(format!("{field}: {state}")).or_default();
            e.0 += 1;
            e.1.insert(path);
        }
    }
    writeln!(out, "\nRACE .hkx citations (occurrences, distinct paths):")?;
    for (k, (n, set)) in &race_paths {
        writeln!(out, "  {n}\t{}\t{k}", set.len())?;
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
