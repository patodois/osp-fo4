#![forbid(unsafe_code)]
//! Census of the winning `.hkx` files of the manifest's mounts with
//! `fo4_havok`: container outcome by header variant, error codes, root and
//! object classes. Measurement support for `spec/havok-container.md` and
//! `spec/havok-animation.md`; writes plain text.
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
        return Err("anim_census MANIFEST OUT.txt".into());
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
    let mut c = BTreeMap::<String, u64>::new();
    let mut objects = BTreeMap::<String, u64>::new();
    let mut files = BTreeMap::<String, u64>::new();
    let mut bytes_total = 0u64;
    for p in &paths {
        let name = String::from_utf8_lossy(p).into_owned();
        let b = Budget::new(m.limits.max_memory);
        let bytes = match vfs.read_path_bytes(p, &b) {
            Ok(v) => v,
            Err(e) => {
                bump(&mut c, format!("file read error {}", e.code));
                continue;
            }
        };
        bytes_total += bytes.len() as u64;
        let extra = bytes.get(60..64).map_or(u32::MAX, |v| {
            u32::from_le_bytes([v[0], v[1], v[2], v[3]]) >> 16
        });
        match fo4_havok::Container::read(&bytes, &name, 0, &b) {
            Ok(k) => {
                bump(
                    &mut c,
                    format!("container read, word 60 upper half {extra}"),
                );
                bump(
                    &mut c,
                    format!(
                        "container read, bytes after the header {}",
                        k.header_extra
                            .iter()
                            .map(|x| format!("{x:02x}"))
                            .collect::<String>()
                    ),
                );
                bump(
                    &mut c,
                    format!(
                        "container read, root {}",
                        k.root().map_or("<none>", |o| o.class)
                    ),
                );
                bump(&mut c, format!("container read, fill {:?}", k.fill));
                let mut seen = BTreeSet::new();
                for o in &k.objects {
                    bump(&mut objects, o.class.to_string());
                    seen.insert(o.class);
                }
                for s in seen {
                    bump(&mut files, s.to_string());
                }
            }
            Err(e) => bump(
                &mut c,
                format!(
                    "container error, word 60 upper half {extra}: {} at {}: expected {}",
                    e.code, e.source.offset, e.expected
                ),
            ),
        }
    }
    let mut out = String::new();
    writeln!(
        out,
        "winning .hkx paths\t{}\nbytes\t{bytes_total}",
        paths.len()
    )?;
    for (k, n) in &c {
        writeln!(out, "{n}\t{k}")?;
    }
    writeln!(out, "\nclass\tfiles\tobjects")?;
    for (k, n) in &files {
        writeln!(out, "{k}\t{n}\t{}", objects[k])?;
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
