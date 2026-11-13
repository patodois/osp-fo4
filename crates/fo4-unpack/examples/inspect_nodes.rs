#![forbid(unsafe_code)]
//! Bounded structural NIF node reports, without vertex payloads or runtime execution.
#[allow(dead_code)]
#[path = "support/mod.rs"]
mod files;
use fo4_core::{Budget, FileSource, Limits, ReadAt};
use std::path::Path;
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("inspect_nodes INPUT_PATHS_JSON build/OUTPUT".into());
    }
    let b = Budget::new(512 << 20);
    let limits = Limits::default();
    let source = FileSource::open(Path::new(&args[0]))?;
    fo4_core::limit(source.len(), 1 << 20, "node input list", 0)?;
    b.charge(
        fo4_core::mul(source.len(), 16, "node input list", 0)?,
        "node input list",
        0,
    )?;
    let paths: Vec<String> = serde_json::from_slice(&fo4_core::read_bytes(
        &source,
        0,
        source.len(),
        &limits,
        &b,
    )?)?;
    fo4_core::limit(paths.len() as u64, 128, "node input count", 0)?;
    let out = files::output_dir(&args[1])?;
    for (ordinal, path) in paths.iter().enumerate() {
        files::memory()?;
        let work = b.child();
        let source = FileSource::open(Path::new(path))?;
        fo4_core::limit(source.len(), 64 << 20, path, 0)?;
        let bytes = fo4_core::read_bytes(&source, 0, source.len(), &limits, &work)?;
        let n = fo4_nif::Nif::read(&bytes, path, fo4_core::Game::Fallout4, &limits, &work)?;
        work.charge(fo4_core::mul(bytes.len() as u64, 16, path, 0)?, path, 0)?;
        let mut nodes = Vec::new();
        work.reserve(&mut nodes, n.blocks.len(), path, 0)?;
        for (i, block) in n.blocks.iter().enumerate() {
            let children = match &block.decoded {
                Some(fo4_nif::Data::Node { children, .. }) => Some(children),
                _ => None,
            };
            nodes.push(serde_json::json!({"index":i,"type":n.block_types[block.type_index as usize],"range":block.data,"fields":block.decoded.as_ref().map(fo4_scene::model_policy::fields),"children":children}));
        }
        files::json(
            &out.join(format!("{ordinal}.json")),
            &serde_json::json!({"input_sha256":fo4_core::digest_bytes(&bytes),"strings":n.strings,"roots":n.roots,"blocks":nodes}),
        )?;
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
