#![forbid(unsafe_code)]
//! Copies every winning `.hkx` file of the manifest's mounts into an output
//! directory outside the repository, as `NNNNN.hkx` with an `index.tsv` of
//! (number, size, archive path), so that measurement scripts can read them
//! without the archives. Measurement support only; the output holds game
//! data and must never be committed.
use fo4_core::{Budget, Manifest};
use fo4_vfs::Vfs;
use std::{fmt::Write as _, path::Path};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("hkx_dump MANIFEST OUT_DIR".into());
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    let out = Path::new(&args[1]);
    std::fs::create_dir_all(out)?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let mut paths: Vec<Vec<u8>> = vfs
        .candidates
        .iter()
        .filter_map(|c| c.key.clone())
        .filter(|k| k.ends_with(b".hkx"))
        .collect();
    paths.sort();
    paths.dedup();
    let mut index = String::new();
    for (i, p) in paths.iter().enumerate() {
        let b = Budget::new(m.limits.max_memory);
        let name = String::from_utf8_lossy(p);
        match vfs.read_path_bytes(p, &b) {
            Ok(bytes) => {
                std::fs::write(out.join(format!("{i:05}.hkx")), &bytes)?;
                writeln!(index, "{i}\t{}\t{name}", bytes.len())?;
            }
            Err(e) => writeln!(index, "{i}\t-1\t{name}\t{}", e.code)?,
        }
    }
    std::fs::write(out.join("index.tsv"), index)?;
    eprintln!("{} paths", paths.len());
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
