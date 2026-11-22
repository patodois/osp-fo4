#![forbid(unsafe_code)]
//! Census of the sound data of one installation with `fo4_audio`
//! (`spec/audio.md`). Measurement support.
//!
//! `audio_census MANIFEST OUT_DIR [CELL[:RADIUS]...]` reads every entry of
//! every mounted archive (one entry at a time; no archive is held in
//! memory) and lists the loose files under the data root, then the records
//! that tie sounds to the world and, for each cell given (an editor id, with
//! the exterior cells within RADIUS grid steps), what the cell places and
//! assigns. It writes `audio-census.txt` (counts, sizes, field values and
//! histograms only; the cells are numbered, not named; committed as
//! `spec/audio-census.txt`) and `audio-census-names.txt` (cell names, record
//! editor ids, file paths and refused files; never versioned).
use fo4_core::{Budget, Manifest};
use fo4_plugin::typed::Store;
use fo4_vfs::Vfs;
use std::{collections::BTreeMap, fmt::Write as _, path::Path, time::Instant};

mod files;
mod records;

pub type Counts = BTreeMap<String, u64>;
pub fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}
pub fn add(m: &mut Counts, k: impl Into<String>, n: u64) {
    *m.entry(k.into()).or_default() += n;
}
/// Holds and fails of a named check.
pub type Checks = BTreeMap<String, [u64; 2]>;
pub fn check(m: &mut Checks, k: impl Into<String>, ok: bool) {
    m.entry(k.into()).or_default()[usize::from(!ok)] += 1;
}

/// Count, minimum, maximum and sum of a value.
#[derive(Default, Clone, Copy)]
pub struct Range {
    pub n: u64,
    pub min: u64,
    pub max: u64,
    pub sum: u64,
}
impl Range {
    pub fn put(&mut self, v: u64) {
        if self.n == 0 || v < self.min {
            self.min = v;
        }
        self.max = self.max.max(v);
        self.n += 1;
        self.sum += v;
    }
}
impl std::fmt::Display for Range {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "n {} min {} max {} sum {}",
            self.n, self.min, self.max, self.sum
        )
    }
}

pub fn section(out: &mut String, title: &str) {
    let _ = writeln!(out, "\n# {title}");
}
pub fn counts(out: &mut String, title: &str, m: &Counts) {
    section(out, title);
    for (k, v) in m {
        let _ = writeln!(out, "{v}\t{k}");
    }
}
pub fn checks(out: &mut String, title: &str, m: &Checks) {
    section(out, &format!("{title} (holds, fails)"));
    for (k, [h, f]) in m {
        let _ = writeln!(out, "{h}\t{f}\t{k}");
    }
}

/// Lower-case four-character code for reports, printable bytes only.
pub fn fourcc(b: &[u8]) -> String {
    b.iter()
        .map(|c| {
            if c.is_ascii_graphic() || *c == b' ' {
                *c as char
            } else {
                '.'
            }
        })
        .collect()
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        return Err("audio_census MANIFEST OUT_DIR [CELL[:RADIUS]...]".into());
    }
    let start = Instant::now();
    let m = Manifest::load(Path::new(&args[0]))?;
    let out_dir = Path::new(&args[1]);
    std::fs::create_dir_all(out_dir)?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let mut out = String::new();
    let mut names = String::new();
    writeln!(
        out,
        "# audio census: {} plugins, {} archive mounts",
        m.plugins.len(),
        vfs.archives.len()
    )?;
    let index = files::run(&m, &vfs, &mut out, &mut names)?;
    let file_seconds = start.elapsed().as_secs_f64();
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let cells: Vec<(String, u32)> = args[2..]
        .iter()
        .map(|a| match a.split_once(':') {
            Some((c, r)) => Ok((c.to_string(), r.parse::<u32>()?)),
            None => Ok((a.clone(), 0)),
        })
        .collect::<Result<_, std::num::ParseIntError>>()?;
    records::run(&m, &store, &index, &cells, &mut out, &mut names)?;
    section(&mut out, "timing");
    writeln!(out, "files pass wall seconds\t{file_seconds:.1}")?;
    writeln!(
        out,
        "total wall seconds\t{:.1}",
        start.elapsed().as_secs_f64()
    )?;
    std::fs::write(out_dir.join("audio-census.txt"), out)?;
    std::fs::write(out_dir.join("audio-census-names.txt"), names)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
