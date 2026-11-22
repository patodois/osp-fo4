#![forbid(unsafe_code)]
//! Census of the navigation data of one installation with `fo4_nav`
//! (`spec/navmesh.md`). Measurement support.
//!
//! `nav_census MANIFEST OUT_DIR` reads every `NAVM` occurrence of the load
//! order (winning and overridden, one record at a time), every `NAVI`
//! occurrence, then checks the winning live meshes against each other and
//! against the cells, placed references and terrain the project already
//! reads. It writes `navmesh-census.txt` (counts, sizes, field values and
//! histograms only; committed as `spec/navmesh-census.txt`) and
//! `navmesh-census-names.txt` (refused records with their file and offset;
//! never versioned).
use fo4_core::{Budget, Manifest};
use fo4_plugin::typed::Store;
use std::{fmt::Write as _, path::Path, time::Instant};

mod geom;
mod info;
mod meshes;
mod stats;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("nav_census MANIFEST OUT_DIR".into());
    }
    let start = Instant::now();
    let m = Manifest::load(Path::new(&args[0]))?;
    let out_dir = Path::new(&args[1]);
    std::fs::create_dir_all(out_dir)?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let mut out = String::new();
    let mut names = String::new();
    writeln!(out, "# navmesh census: {} plugins", m.plugins.len())?;
    writeln!(
        out,
        "store diagnostics (records the plugin index could not place)\t{}",
        store.diagnostics.len()
    )?;
    let summary = meshes::run(&m, &store, &mut out, &mut names)?;
    info::run(&m, &store, &summary, &mut out, &mut names)?;
    stats::section(&mut out, "timing");
    writeln!(
        out,
        "total wall seconds\t{:.1}",
        start.elapsed().as_secs_f64()
    )?;
    std::fs::write(out_dir.join("navmesh-census.txt"), out)?;
    std::fs::write(out_dir.join("navmesh-census-names.txt"), names)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
